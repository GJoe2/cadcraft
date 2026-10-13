//! Integration tests for raster image paste and canvas rendering.

use cadcraft_doc::{EntityKind, Image};
use cadcraft_engine::Session;
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_ui_egui::{CadApp, Services};

fn create_test_png() -> Vec<u8> {
    let mut buf = Vec::new();
    let img = image::RgbaImage::from_fn(64, 48, |x, y| image::Rgba([(x * 4) as u8, (y * 5) as u8, 128, 255]));
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).expect("write test png");
    buf
}

#[test]
fn pasteclip_inserts_image_with_cached_asset() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let _ = app.session.cmdline("new metric");

    let png_bytes = create_test_png();
    app.session.set_clipboard_image(png_bytes.clone(), 64, 48);

    // Paste at insertion point (100, 200)
    let res = app.session.execute(
        "pasteclip",
        &serde_json::json!({
            "at": [100.0, 200.0],
        }),
    );
    assert!(res.is_ok(), "pasteclip execution failed: {res:?}");

    let st = app.session.state().expect("state");
    assert_eq!(st.doc.model.len(), 1, "image entity should be present in model space");

    let ent = st.doc.model.iter().next().expect("entity");
    if let EntityKind::Image(img) = &ent.kind {
        assert_eq!(img.insert, Vec3::new(100.0, 200.0, 0.0));
        assert_eq!(img.size, Vec2::new(64.0, 48.0));
        assert!(img.path.starts_with("paste_"));
        assert!(img.path.ends_with(".png"));

        // Verify the asset bytes are embedded in Drawing.image_assets
        let stored = st.doc.get_image_asset(&img.path);
        assert!(stored.is_some(), "image asset must be stored in drawing");
        assert_eq!(stored.unwrap().as_slice(), png_bytes.as_slice());
    } else {
        panic!("expected EntityKind::Image, got {:?}", ent.kind);
    }
}

#[test]
fn hostile_and_missing_images_never_panic() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let _ = app.session.cmdline("new metric");

    // Store a corrupted asset
    let h = {
        let d = app.session.doc_mut().expect("doc_mut");
        d.store_image_asset("corrupt.png", vec![0xFF, 0x00, 0x12, 0x34]);
        let h = d.new_handle();
        let e = cadcraft_doc::Entity::new(
            h,
            EntityKind::Image(Image {
                insert: Vec3::ZERO,
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
                size: Vec2::new(100.0, 100.0),
                path: "corrupt.png".into(),
                name: "corrupt.png".into(),
                ..Image::default()
            }),
        );
        d.model.push(e);

        // Also add an entity referencing a non-existent disk file
        let h2 = d.new_handle();
        let e2 = cadcraft_doc::Entity::new(
            h2,
            EntityKind::Image(Image {
                insert: Vec3::new(500.0, 500.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
                size: Vec2::new(50.0, 50.0),
                path: "/non_existent/path/to/missing_file.jpg".into(),
                name: "missing".into(),
                ..Image::default()
            }),
        );
        d.model.push(e2);
        h
    };

    // Attempting commands and operations on corrupted/missing images must never crash
    let res = app.session.execute(
        "imageadjust",
        &serde_json::json!({
            "handles": [h.hex()],
            "brightness": 70,
            "contrast": 80,
        }),
    );
    assert!(res.is_ok(), "imageadjust failed: {res:?}");

    let res = app.session.execute("imageframe", &serde_json::json!({ "mode": 1 }));
    assert!(res.is_ok());
}

#[test]
fn os_clipboard_can_initialize_on_system() {
    if std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok() {
        let cb = arboard::Clipboard::new();
        assert!(cb.is_ok(), "arboard clipboard must initialize when display is set: {:?}", cb.as_ref().err());
    }
}

#[test]
fn try_load_os_clipboard_image_does_not_panic() {
    let mut app = CadApp::new(Session::new(), Services::default());
    // Calling try_load_os_clipboard_image must never panic
    let _ = cadcraft_ui_egui::clipboard::try_load_os_clipboard_image(&mut app);
}

#[test]
fn wsl_clipboard_image_can_be_retrieved_and_pasted() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let _ = app.session.cmdline("new metric");

    let loaded = cadcraft_ui_egui::clipboard::try_load_os_clipboard_image(&mut app);
    if loaded {
        assert!(app.session.clipboard_image.is_some());
        let res = app.session.execute(
            "pasteclip",
            &serde_json::json!({
                "at": [0.0, 0.0],
            }),
        );
        assert!(res.is_ok());
        let st = app.session.state().expect("state");
        assert_eq!(st.doc.model.len(), 1);
    }
}

#[test]
fn imageattach_from_menu_picker_does_not_loop_and_prompts_insertion_point() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = calls.clone();

    // Create a temporary png file
    let tmp = std::env::temp_dir().join("cadcraft_test_attach.png");
    let png_bytes = create_test_png();
    std::fs::write(&tmp, &png_bytes).expect("write temp png");

    let tmp_path = tmp.to_string_lossy().to_string();
    let services = Services {
        pick_image: Some(Box::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            Some(tmp_path.clone())
        })),
        ..Services::default()
    };

    let mut app = CadApp::new(Session::new(), services);
    let _ = app.session.cmdline("new metric");

    // Starting imageattach via app.start("imageattach") (like selecting it from menu)
    app.start("imageattach");

    // Verify pick_image was called exactly ONCE, not in an infinite loop
    assert_eq!(calls.load(Ordering::SeqCst), 1, "pick_image must be called exactly once");

    // Verify the command is running and prompting for insertion point
    let prompt = app.session.current_prompt().expect("current prompt");
    assert!(prompt.message.contains("insertion point"), "prompt should be insertion point, got: {}", prompt.message);

    // Provide insertion point, scale, rotation
    let _ = app.session.input(cadcraft_engine::Input::Point(Vec2::new(10.0, 20.0)));
    let _ = app.session.input(cadcraft_engine::Input::Text("1.0".into()));
    let _ = app.session.input(cadcraft_engine::Input::Text("0".into()));

    // Verify the image was placed
    let st = app.session.state().expect("state");
    assert_eq!(st.doc.model.len(), 1, "image entity should be attached");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn pasteclip_image_shows_live_preview_and_pastes_interactively() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let _ = app.session.cmdline("new metric");

    let png_bytes = create_test_png();
    app.session.set_clipboard_image(png_bytes.clone(), 64, 48);

    // Start pasteclip interactively on session
    app.session.start("pasteclip").expect("start pasteclip");

    let prompt = app.session.current_prompt().expect("prompt");
    assert!(prompt.message.contains("insertion point"));

    // Verify drag is None for image paste so preview is not suppressed
    let drag = app.session.drag(Vec2::new(30.0, 40.0));
    assert!(drag.is_none(), "drag must be None for image clipboard paste to allow preview");

    // Verify preview produces an image entity with path "preview"
    let preview_ents = app.session.preview(Vec2::new(30.0, 40.0));
    assert_eq!(preview_ents.len(), 1, "preview should return 1 entity");
    if let EntityKind::Image(img) = &preview_ents[0].kind {
        assert_eq!(img.insert, Vec3::new(30.0, 40.0, 0.0));
        assert_eq!(img.path, "preview");
        assert_eq!(img.size, Vec2::new(64.0, 48.0));
    } else {
        panic!("preview entity must be Image, got: {:?}", preview_ents[0].kind);
    }

    // Input point to finish paste
    let _ = app.session.input(cadcraft_engine::Input::Point(Vec2::new(30.0, 40.0)));

    let st = app.session.state().expect("state");
    assert_eq!(st.doc.model.len(), 1, "image should be placed in document");
}
