//! Scoped packet capture for a tentative server action. Reserve each body's retained bytes as
//! it enters the batch; rollback releases reservations, commit transfers them to normal output.
use super::{Body, PlayerSender, io_pool};
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::atomic::Ordering};

thread_local! {
    static SCOPES: RefCell<Vec<Packets>> = const { RefCell::new(Vec::new()) };
}

/// Encoded packets in send order, bound to their original connections.
#[derive(Default)]
pub struct Packets {
    bodies: Vec<ReservedBody>,
    failed: bool,
}

// The journal keeps a sender as well as a body per entry, and grows geometrically.
const JOURNAL_CHARGE: usize = 2 * std::mem::size_of::<ReservedBody>();

struct ReservedBody {
    sender: PlayerSender,
    body: Option<Body>,
    cost: usize,
}

impl Drop for ReservedBody {
    fn drop(&mut self) {
        if self.body.is_some() {
            self.sender.pending.fetch_sub(self.cost, Ordering::AcqRel);
        }
    }
}

impl Packets {
    /// Publish only after the outermost transaction has committed.
    pub fn publish(self) {
        debug_assert!(!self.failed);
        for mut packet in self.bodies {
            let body = packet.body.take().expect("reserved body");
            packet
                .sender
                .pending
                .fetch_sub(JOURNAL_CHARGE, Ordering::AcqRel);
            packet.sender.publish_reserved(body);
        }
    }
}

/// A game-thread scope. Dropping it without `finish` discards tentative packets.
pub struct Scope {
    open: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Scope {
    /// Begin capture before any tentative operation can send a packet.
    pub fn begin() -> Self {
        SCOPES.with(|s| s.borrow_mut().push(Packets::default()));
        Self {
            open: true,
            _thread: PhantomData,
        }
    }
    /// Transactions nested inside a capturing action also isolate their packets.
    pub fn nested() -> Option<Self> {
        active().then(Self::begin)
    }
    /// Whether the current batch ran out of retained-byte budget.
    pub fn refused() -> bool {
        SCOPES.with(|s| s.borrow().last().is_some_and(|packets| packets.failed))
    }
    /// Merge a successful child in send order, or transfer the outer scope to the journal.
    pub fn finish(mut self) -> Option<Packets> {
        self.open = false;
        SCOPES.with(|s| {
            let mut scopes = s.borrow_mut();
            let packets = scopes.pop().expect("packet scope is current");
            debug_assert!(!packets.failed);
            match scopes.last_mut() {
                Some(parent) => {
                    parent.bodies.extend(packets.bodies);
                    None
                }
                None => Some(packets),
            }
        })
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if self.open {
            SCOPES.with(|s| {
                s.borrow_mut().pop().expect("packet scope is current");
            });
        }
    }
}

pub(super) fn active() -> bool {
    SCOPES.with(|s| !s.borrow().is_empty())
}

pub(super) fn capture(sender: &PlayerSender, body: Body) -> Result<(), Body> {
    SCOPES.with(|s| {
        let mut scopes = s.borrow_mut();
        let Some(packets) = scopes.last_mut() else {
            return Err(body);
        };
        if packets.failed {
            return Ok(());
        }
        let cost = io_pool::charge(body.retained()).saturating_add(JOURNAL_CHARGE);
        if sender.reserve(cost) {
            packets.bodies.push(ReservedBody {
                sender: sender.clone(),
                body: Some(body),
                cost,
            });
        } else {
            // Refuse the complete take even if this child is abandoned before it can merge.
            for packets in &mut *scopes {
                packets.failed = true;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan::conn::Out;
    use std::sync::{Arc, mpsc};

    #[test]
    fn reserves_nested_bodies_in_order_and_refunds_refused_batches() {
        let (tx, rx) = mpsc::channel();
        let mut sender = PlayerSender::new(tx, Arc::default());
        let charge = io_pool::charge(1);
        sender.cap = 3 * (charge + JOURNAL_CHARGE);

        let outer = Scope::begin();
        sender.send(vec![1]);
        let child = Scope::nested().unwrap();
        sender.send(vec![0]);
        assert_eq!(sender.queued().1, 2 * (charge + JOURNAL_CHARGE));
        drop(child);
        assert_eq!(sender.queued().1, charge + JOURNAL_CHARGE);
        let child = Scope::nested().unwrap();
        sender.send(vec![2]);
        assert!(child.finish().is_none());
        sender.send(vec![3]);
        assert!(rx.try_recv().is_err());
        outer.finish().unwrap().publish();
        assert_eq!(sender.queued().1, 3 * charge);
        for expected in 1..=3 {
            assert!(matches!(rx.try_recv().unwrap(), Out::Packet(body) if body == [expected]));
        }
        sender.pending.store(0, Ordering::Relaxed); // The test channel has no network drain.

        let outer = Scope::begin();
        sender.send(vec![4]);
        let child = Scope::nested().unwrap();
        sender.send(vec![5]);
        sender.send(vec![6]);
        sender.send(vec![7]);
        assert!(Scope::refused());
        drop(child);
        assert!(
            Scope::refused(),
            "refusal is not hidden by a child rollback"
        );
        drop(outer);
        assert_eq!(sender.queued().1, 0);
        assert!(rx.try_recv().is_err());
    }
}
