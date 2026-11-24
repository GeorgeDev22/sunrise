//! Chunk attachment snapshot encoding boundary.

pub trait Hooks {
    /// A chunk snapshot has been captured on the owning server thread, before
    /// background encoding. The callback runs on that server thread.
    /// `data` belongs only to that snapshot: typed edits change the disk record,
    /// never the live chunk. Exactly once per captured record, for full and proto
    /// chunks alike. The restricted Host cannot resolve live chunks or mutate the
    /// world; `host.job()` borrows the captured metadata. Codec and key errors refuse
    /// only that operation. A listener that panics loses only its edits and is disabled
    /// for this hook for the session. Other listeners' edits still reach the ordinary
    /// disk writer, whose limits and failure handling are unchanged.
    #[hook(
        side = "server",
        contract = "listen",
        context = "chunk_disk",
        target = "mod_api::chunk_disk::native_chunk_data_save",
        neoforge = "ChunkDataEvent.Save",
        version = 4
    )]
    fn chunk_data_save(data: &mut ChunkDiskData);
}
