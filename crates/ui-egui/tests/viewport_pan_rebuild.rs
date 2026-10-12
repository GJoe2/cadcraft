use cadcraft_doc::{Common, EntityKind, Space, Viewport};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

#[test]
fn pan_inside_active_viewport_rebuilds_display_list() {
    let mut session = Session::new();
    session.new_drawing(true);
    session.viewport_px = (1600.0, 1000.0);
    session.execute("line", &json!({"points": [[0, 0], [1000, 0]]})).unwrap();
    session.execute("line", &json!({"points": [[0, 0], [0, 500]]})).unwrap();
    session.execute("line", &json!({"points": [[500, 200], [500, 300]]})).unwrap();

    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        id: 2,
        center: cadcraft_geom::Vec3::new(70.0, 60.0, 0.0),
        width: 100.0,
        height: 80.0,
        view_center: Vec2::new(500.0, 250.0),
        view_height: 500.0,
        ..Default::default()
    };
    let vh = session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();

    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    session.execute("mspace", &json!({"handle": vh.hex()})).unwrap();

    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;

    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);

    let gen_before = h.state().canvas.list_gen;
    let list_before = h.state().canvas.list.as_ref().expect("canvas display list should be built");
    let verts_before = list_before.verts.clone();

    // Panning inside the unlocked viewport must bump revision and rebuild the display list
    h.state_mut().session.execute("pan", &json!({"delta": [50.0, 0.0]})).unwrap();
    h.run_steps(3);

    let gen_after = h.state().canvas.list_gen;
    assert!(gen_after > gen_before, "panning inside active layout viewport must rebuild the display list (before={gen_before}, after={gen_after})");
    let list_after = h.state().canvas.list.as_ref().expect("canvas display list should be built");
    let verts_after = list_after.verts.clone();
    assert_ne!(verts_before, verts_after, "viewport content vertices in display list must shift on pan");
}
