//! A pack's Polytone expression uniforms on its core shaders (`shader_modifiers`) through a
//! game's frames, from the pack's resources to what is drawn: the menu bundle's atlas loads each
//! shader and resolves its uniforms from the pack's files, the renderer the game builds from it
//! takes both, and each frame evaluates the programs into the blocks the shaders read. F3's
//! lightmap chart reads the drawn light texture back; a screenshot shows the sky. Each case runs
//! in a process of its own (one event loop, its own game resources).
use super::native_entity_refs::tests::{run_fresh_cases, with_client_game};
use crate::game::native_hooks::particle_count_tests::frame_scene;
use sunrise_assets::resource_source::{Resource, ResourceSource};

const NAME: &str = "game::lightmap_expression_tests::a_packs_lightmap_draws_its_expression_uniform";
const CASE: &str = "SUNRISE_LIGHTMAP_EXPRESSION_CASE";

/// A pack whose lightmap writes its `Tint` expression uniform (0.2, a value a unorm byte holds
/// exactly: 51) in red and nothing else, with the file that gives it.
fn pack() -> ResourceSource {
    let files = [
        (
            "assets/minecraft/shaders/core/screenquad.vsh",
            "#version 330\nlayout(location = 0) out vec2 texCoord;\nvoid main() {\n    \
             vec2 uv = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);\n    \
             gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);\n    texCoord = uv;\n}\n",
        ),
        (
            "assets/minecraft/shaders/core/lightmap.fsh",
            "#version 330\nlayout(std140) uniform Tint {\n    float value;\n};\n\
             layout(location = 0) in vec2 texCoord;\nlayout(location = 0) out vec4 fragColor;\n\
             void main() {\n    fragColor = vec4(value, 0.0, 0.0, 1.0);\n}\n",
        ),
        (
            "assets/minecraft/polytone/shader_modifiers/core/lightmap.json",
            r#"{"Tint": "0.1 * 2"}"#,
        ),
    ];
    ResourceSource::indexed(
        files
            .into_iter()
            .map(|(path, text)| (path.to_owned(), Resource::bytes(text.as_bytes().to_vec())))
            .collect(),
    )
}

/// A pack whose sky writes its `Tint` (0.2: 51) in red, from its own file, and whose stars and
/// block outline read a `Tint` of their own files (0.5, 0.75): those of `shaders` only.
fn sky_pack(shaders: &[&str]) -> ResourceSource {
    let vsh = "#version 330\nlayout(location = 0) in vec3 Position;\nvoid main() {\n    \
               gl_Position = vec4(Position.x / 300.0, Position.z / 300.0, 0.0, 1.0);\n}\n";
    let fsh = "#version 330\nlayout(std140) uniform Tint {\n    float value;\n};\n\
               layout(location = 0) out vec4 fragColor;\n\
               void main() {\n    fragColor = vec4(value, 0.0, 0.0, 1.0);\n}\n";
    let mut files: Vec<(String, String)> = Vec::new();
    let all = [
        ("sky", "0.2"),
        ("stars", "0.5"),
        ("rendertype_lines", "0.75"),
    ];
    for (shader, value) in all.into_iter().filter(|(s, _)| shaders.contains(s)) {
        let core = format!("assets/minecraft/shaders/core/{shader}");
        files.push((format!("{core}.vsh"), vsh.to_owned()));
        files.push((format!("{core}.fsh"), fsh.to_owned()));
        let modifier = format!("assets/minecraft/polytone/shader_modifiers/core/{shader}.json");
        files.push((modifier, format!(r#"{{"Tint": "{value}"}}"#)));
    }
    ResourceSource::indexed(
        files
            .into_iter()
            .map(|(path, text)| (path, Resource::bytes(text.into_bytes())))
            .collect(),
    )
}

/// The light texture F3's lightmap chart reads back once frames have drawn it, with the chart
/// open (the game asks the renderer for the readback each frame): a frame copies it, the device
/// maps the copy once the frame is done, and a later frame takes it in.
fn drawn_texture(game: &mut super::Game<'_>) -> [u8; 16 * 16 * 4] {
    game.screen = super::Screen::Playing;
    game.debug = true;
    game.debug_charts.lightmap = true;
    for _ in 0..32 {
        game.render_with(|renderer, frame| renderer.render(frame))
            .unwrap();
        let renderer = game.renderer.as_ref().unwrap();
        renderer
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        if let Some(pixels) = renderer.lightmap_sample() {
            return *pixels;
        }
    }
    panic!("no light texture read back in 32 frames");
}

#[test]
#[ignore = "daily"]
fn a_packs_lightmap_draws_its_expression_uniform() {
    if run_fresh_cases(NAME, CASE, &["pack", "fullbright", "sky", "outline"]) {
        return;
    }
    let case = std::env::var(CASE).unwrap();
    if case == "sky" {
        return the_sky_draws_its_expression_uniform();
    }
    if case == "outline" {
        // An outline-only modifier is enough to enable expression evaluation.
        return with_client_game(|game| {
            let pack = sky_pack(&["rendertype_lines"]);
            game.gui = crate::game::UiResources::load(&pack, std::path::Path::new("")).unwrap();
            let _event_loop = frame_scene(game);
            assert!(game.reads_pack_expressions());
        });
    }
    let fullbright = case == "fullbright";
    with_client_game(|game| {
        // The pack's resources as the game loads them: the atlas resolves the uniform, and the
        // renderer the game builds over them takes the shader with it.
        game.gui = crate::game::UiResources::load(&pack(), std::path::Path::new("")).unwrap();
        assert!(game.gui.atlas.lightmap_shader.is_some());
        assert_eq!(game.gui.atlas.lightmap_expressions.len(), 1);
        let _event_loop = frame_scene(game);
        game.renderer.as_mut().unwrap().fullbright = fullbright;
        let texture = drawn_texture(game);
        if fullbright {
            // Fullbright keeps the texture white, and nothing is evaluated.
            assert!(!game.reads_pack_expressions());
            assert!(texture.iter().all(|&b| b == 255));
        } else {
            assert!(game.reads_pack_expressions());
            for texel in texture.chunks(4) {
                assert_eq!(texel, [51, 0, 0, 255]);
            }
        }
    });
}

/// The sky case: the pack's sky, stars and outline take their uniforms from their own files
/// (the same name in each), and the sky disc, which covers the frame (its rim past the screen's
/// edges) draws its value.
fn the_sky_draws_its_expression_uniform() {
    with_client_game(|game| {
        game.gui = crate::game::UiResources::load(
            &sky_pack(&["sky", "stars", "rendertype_lines"]),
            std::path::Path::new(""),
        )
        .unwrap();
        let atlas = &game.gui.atlas;
        let values = |expressions: &[sunrise_client::polytone_post::ExpressionUniform]| {
            let frame = sunrise_client::environment::Frame::default();
            expressions
                .iter()
                .map(|e| e.evaluate(&frame))
                .collect::<Vec<_>>()
        };
        assert_eq!(values(&atlas.sky_expressions), [0.2]);
        assert_eq!(values(&atlas.stars_expressions), [0.5]);
        assert_eq!(values(&atlas.lines_expressions), [0.75]);
        let _event_loop = frame_scene(game);
        assert!(game.reads_pack_expressions());
        game.renderer.as_mut().unwrap().screenshot = Some(sunrise_client::gfx::ScreenshotRequest {
            path: "unused.png".into(),
            size: None,
            scaled: false,
            background: false,
        });
        game.render_with(|renderer, frame| renderer.render(frame))
            .unwrap();
        let result = game.renderer.as_mut().unwrap().screenshot_result.take();
        let mut pending = result.unwrap().unwrap();
        let image = pending.image_mut();
        let width = image.width as usize;
        let corner = &image.rgba[(2 * width + 2) * 4..(2 * width + 2) * 4 + 3];
        assert_eq!(corner, [51, 0, 0]);
    });
}
