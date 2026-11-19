//! A pack core shader pipeline's blocks: vanilla's four bound only to the stages that read them,
//! and its Polytone expression uniforms (`shader_modifiers`, wiki "Expression Uniforms on Any
//! Shader"), resolved when the shader loaded (`polytone_post::ShaderModifiers::resolve`), each a
//! block of one float after the pipeline's own, holding its program's value this frame. The
//! lightmap, sky, stars and block outline pipelines lay theirs out with this.

use crate::core_shader::CoreProgram;
use crate::polytone_post::ExpressionUniform;

/// The block outline's shader, as a pack's `shader_modifiers` names it.
pub const LINES: [&str; 1] = ["minecraft:core/rendertype_lines"];
/// The sky disc's and the stars' shaders.
pub const SKY: [&str; 1] = ["minecraft:core/sky"];
pub const STARS: [&str; 1] = ["minecraft:core/stars"];

/// An expression uniform's block: one float, as std140 rounds a block.
const BLOCK: u64 = 16;

/// Each vanilla uniform block's offset in a pipeline's buffer (`core_shader::BLOCKS` order).
pub(crate) const VANILLA_OFFSETS: [u64; 4] = [0, 256, 512, 768];
pub(crate) const VANILLA_SIZES: [u64; 4] = [48, 64, 160, 48];

/// Vanilla's blocks a stage of `program` reads: binding, offset, size and the stages reading it.
pub(crate) fn vanilla_blocks(program: &CoreProgram) -> Vec<(u32, u64, u64, wgpu::ShaderStages)> {
    (0..4u32)
        .map(|b| {
            let i = b as usize;
            (
                b,
                VANILLA_OFFSETS[i],
                VANILLA_SIZES[i],
                program.stages_reading(b),
            )
        })
        .filter(|(.., stages)| !stages.is_empty())
        .collect()
}

/// A layout entry for a uniform block at `binding`, visible to `stages`, at least `size` bytes.
pub(crate) fn block_entry(
    binding: u32,
    size: u64,
    stages: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: stages,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    }
}

/// The expression uniforms of one pipeline, laid out in its uniform buffer from `base`, 256 bytes
/// apart (the uniform offset alignment), with this frame's values.
#[derive(Debug)]
pub(crate) struct ExpressionBlocks {
    uniforms: Vec<(ExpressionUniform, f32)>,
    base: u64,
}

impl ExpressionBlocks {
    /// `uniforms` (the blocks its stages read) from `base` in the pipeline's buffer.
    pub(crate) fn new(uniforms: &[ExpressionUniform], base: u64) -> Self {
        Self {
            uniforms: uniforms.iter().map(|u| (u.clone(), 0.0)).collect(),
            base,
        }
    }

    fn offset(&self, i: usize) -> u64 {
        self.base + 256 * i as u64
    }

    /// The uniform buffer's size with these blocks from `base`: `base` without any.
    pub(crate) fn size(&self) -> u64 {
        self.offset(self.uniforms.len())
    }

    /// Their layout entries, each visible to the stages that read it.
    pub(crate) fn layout(&self) -> impl Iterator<Item = wgpu::BindGroupLayoutEntry> + '_ {
        (self.uniforms.iter()).map(|(u, _)| block_entry(u.binding, BLOCK, u.stages))
    }

    pub(crate) fn entries<'a>(
        &'a self,
        buffer: &'a wgpu::Buffer,
    ) -> impl Iterator<Item = wgpu::BindGroupEntry<'a>> + 'a {
        self.uniforms
            .iter()
            .enumerate()
            .map(move |(i, (u, _))| wgpu::BindGroupEntry {
                binding: u.binding,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer,
                    offset: self.offset(i),
                    size: wgpu::BufferSize::new(BLOCK),
                }),
            })
    }

    /// Each block's value this frame, 16 bytes each, into `buffer`.
    pub(crate) fn upload(
        &self,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
    ) {
        for (i, (_, value)) in self.uniforms.iter().enumerate() {
            let mut block = [0u8; BLOCK as usize];
            block[..4].copy_from_slice(&value.to_le_bytes());
            uploads.buffer(encoder, buffer, self.offset(i), &block);
        }
    }

    /// Each value from its program in `frame` (the packs' expression context).
    pub(crate) fn evaluate(&mut self, frame: &crate::environment::Frame) {
        for (uniform, value) in &mut self.uniforms {
            *value = uniform.evaluate(frame);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.uniforms.is_empty()
    }

    /// The uniforms and their values, for tests to set and read.
    #[cfg(test)]
    pub(crate) fn values(&mut self) -> &mut [(ExpressionUniform, f32)] {
        &mut self.uniforms
    }
}

/// The packs' core shader pipelines' expression uniforms, for the game to evaluate.
impl super::Renderer {
    /// The installed pack pipelines whose stages read expression uniforms. Fullbright excludes
    /// the lightmap only. Evaluation does not depend on which pipelines draw this frame.
    fn expression_blocks(&mut self) -> impl Iterator<Item = &mut ExpressionBlocks> {
        let fullbright = self.fullbright;
        let lightmap = (self.core_lightmap.as_mut())
            .filter(|_| !fullbright)
            .map(|core| &mut core.extra);
        let lines = self.core_lines.as_mut().map(|core| &mut core.extra);
        let sky = (self.core_sky.iter_mut()).flat_map(|core| core.expression_blocks());
        (lightmap.into_iter().chain(lines).chain(sky)).filter(|blocks| !blocks.is_empty())
    }

    /// Whether the game needs an expression context for the installed pack pipelines this frame.
    pub fn reads_pack_expressions(&mut self) -> bool {
        self.expression_blocks().next().is_some()
    }

    /// Evaluates the installed pack pipelines' expression uniforms from `frame`. The values
    /// are uploaded separately as each pipeline prepares its draw.
    pub fn evaluate_pack_expressions(&mut self, frame: &crate::environment::Frame) {
        for blocks in self.expression_blocks() {
            blocks.evaluate(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resolved uniform `name` at `binding` that the stages `stages` read, giving `value`.
    fn uniform(
        name: &str,
        binding: u32,
        stages: wgpu::ShaderStages,
        value: &str,
    ) -> ExpressionUniform {
        let file = serde_json::json!({ name: value }).to_string();
        let pack = sunrise_assets::resource_source::ResourceSource::indexed(
            [(
                "assets/minecraft/polytone/shader_modifiers/core/sky.json".to_owned(),
                sunrise_assets::resource_source::Resource::bytes(file.into_bytes()),
            )]
            .into_iter()
            .collect(),
        );
        let modifiers = crate::polytone_post::ShaderModifiers::read(&pack, Default::default());
        modifiers.uniform_for_test(name, binding, stages)
    }

    /// The blocks bind after the pipeline's own, 256 bytes apart from `base`, each visible to
    /// the stages that read it, and hold their programs' values.
    #[test]
    fn expression_blocks_lay_out_and_hold_their_values() {
        let (v, f) = (wgpu::ShaderStages::VERTEX, wgpu::ShaderStages::FRAGMENT);
        let mut blocks = ExpressionBlocks::new(
            &[
                uniform("Tint", 18, f, "0.25"),
                uniform("Fade", 20, v | f, "0.5 * 3"),
            ],
            1024,
        );
        let layout: Vec<(u32, wgpu::ShaderStages)> =
            blocks.layout().map(|e| (e.binding, e.visibility)).collect();
        assert_eq!(layout, [(18, f), (20, v | f)]);
        assert_eq!(blocks.size(), 1024 + 512);
        blocks.evaluate(&Default::default());
        let values: Vec<f32> = blocks.values().iter().map(|(_, v)| *v).collect();
        assert_eq!(values, [0.25, 1.5]);
        let none = ExpressionBlocks::new(&[], 1024);
        assert!(none.is_empty());
        assert_eq!(none.size(), 1024);
    }
}
