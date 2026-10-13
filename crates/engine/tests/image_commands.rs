use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use cadcraft_geom::{Vec2, Vec3};
use serde_json::json;

#[test]
fn imageattach_creates_entity() {
    let mut s = Session::new();
    let r = s
        .execute(
            "imageattach",
            &json!({
                "path": "test_drone_photo.png",
                "at": [25.0, 40.0],
                "scale": 1.5,
                "size": [800.0, 600.0],
            }),
        )
        .unwrap();
    assert_eq!(r["path"], "test_drone_photo.png");

    let doc = s.doc().unwrap();
    assert_eq!(doc.model.len(), 1);
    let ent = doc.model.iter().next().unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        assert_eq!(img.insert, Vec3::new(25.0, 40.0, 0.0));
        assert_eq!(img.size, Vec2::new(800.0, 600.0));
        assert_eq!(img.u, Vec3::new(1.5, 0.0, 0.0));
        assert_eq!(img.v, Vec3::new(0.0, 1.5, 0.0));
    } else {
        panic!("expected Image entity");
    }
}

#[test]
fn imageadjust_modifies_display_settings() {
    let mut s = Session::new();
    let r = s
        .execute(
            "imageattach",
            &json!({
                "path": "test.png",
                "at": [0.0, 0.0],
                "size": [100.0, 100.0],
            }),
        )
        .unwrap();
    let h = r["handle"].as_str().unwrap();

    s.execute(
        "imageadjust",
        &json!({
            "handles": [h],
            "brightness": 75,
            "contrast": 80,
            "fade": 30,
        }),
    )
    .unwrap();

    let doc = s.doc().unwrap();
    let ent = doc.model.iter().next().unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        assert_eq!(img.brightness, 75);
        assert_eq!(img.contrast, 80);
        assert_eq!(img.fade, 30);
    } else {
        panic!("expected Image entity");
    }
}

#[test]
fn imageclip_sets_and_clears_boundary() {
    let mut s = Session::new();
    let r = s
        .execute(
            "imageattach",
            &json!({
                "path": "test.png",
                "at": [0.0, 0.0],
                "size": [100.0, 100.0],
            }),
        )
        .unwrap();
    let h = r["handle"].as_str().unwrap();

    s.execute(
        "imageclip",
        &json!({
            "handle": h,
            "boundary": [[10.0, 10.0], [90.0, 90.0]],
        }),
    )
    .unwrap();

    let doc = s.doc().unwrap();
    let ent = doc.model.iter().next().unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        assert!(img.clipping);
        assert_eq!(img.clip.len(), 2);
    } else {
        panic!("expected Image entity");
    }

    // Clear boundary
    s.execute(
        "imageclip",
        &json!({
            "handle": h,
            "delete": true,
        }),
    )
    .unwrap();

    let doc = s.doc().unwrap();
    let ent = doc.model.iter().next().unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        assert!(!img.clipping);
        assert!(img.clip.is_empty());
    } else {
        panic!("expected Image entity");
    }
}

#[test]
fn normalize_path_handles_windows_and_quoted_paths() {
    use cadcraft_engine::cmd::image::normalize_path;

    // Normal paths
    assert_eq!(normalize_path("image.png"), "image.png");
    assert_eq!(normalize_path("\"/tmp/test.png\""), "/tmp/test.png");
    assert_eq!(normalize_path("'/tmp/test.png'"), "/tmp/test.png");

    #[cfg(unix)]
    {
        // Windows drive paths on Linux/WSL
        let norm = normalize_path(r"C:\Users\foo\bar.png");
        assert_eq!(norm, "/mnt/c/Users/foo/bar.png");

        let norm_fwd = normalize_path("c:/Users/foo/bar.png");
        assert_eq!(norm_fwd, "/mnt/c/Users/foo/bar.png");
    }
}

#[test]
fn imageframe_updates_header_variable() {
    let mut s = Session::new();
    s.execute("imageframe", &json!({ "mode": 2 })).unwrap();
    let doc = s.doc().unwrap();
    let mode = doc.header.get("IMAGEFRAME").and_then(cadcraft_doc::HVal::as_i64).unwrap_or(1);
    assert_eq!(mode, 2);

    s.execute("imageframe", &json!({ "mode": 0 })).unwrap();
    let doc2 = s.doc().unwrap();
    let mode0 = doc2.header.get("IMAGEFRAME").and_then(cadcraft_doc::HVal::as_i64).unwrap_or(1);
    assert_eq!(mode0, 0);
}

#[test]
fn imageattach_with_path_interactive() {
    use cadcraft_engine::Interactive;
    use cadcraft_engine::cmd::image::ImageAttachM;

    let m = ImageAttachM::with_path("/tmp/test.png");
    assert_eq!(m.name(), "IMAGEATTACH");
    let s = Session::new();
    let prompt = m.prompt(&s);
    assert!(prompt.message.contains("insertion point"));
}

#[test]
fn image_grips_allow_scaling_moving_and_rotating() {
    use cadcraft_geom::Vec2;

    let mut s = Session::new();
    let res = s
        .execute(
            "imageattach",
            &json!({
                "path": "test.png",
                "at": [0.0, 0.0],
                "scale": 1.0,
                "rotation": 0.0,
                "size": [100.0, 100.0],
            }),
        )
        .unwrap();
    let h_str = res.get("handle").unwrap().as_str().unwrap();
    let h = cadcraft_doc::Handle::parse_hex(h_str).unwrap();

    let grips = s.grips_of(h).unwrap();
    assert_eq!(grips.len(), 6, "image should have 6 grips: 4 corners, center, top-mid");

    // 1. Stretch corner 2 (top-right, initially at 100, 100) to (200, 200) -> scales image 2x
    s.grip_edit(h, 2, Vec2::new(200.0, 200.0), cadcraft_engine::grips::GripMode::Stretch).unwrap();
    let doc = s.doc().unwrap();
    let ent = doc.entity(h).unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        assert_eq!(img.insert, Vec3::new(0.0, 0.0, 0.0));
        assert!((img.u.x - 2.0).abs() < 1e-9);
        assert!((img.v.y - 2.0).abs() < 1e-9);
        let corners = img.corners();
        assert!(corners[2].near(Vec2::new(200.0, 200.0), 1e-9));
    } else {
        panic!("expected image");
    }

    // 2. Stretch center grip 4 to (50, 50) -> moves entire image by delta
    let center_before = s.grips_of(h).unwrap()[4];
    s.grip_edit(h, 4, center_before + Vec2::new(50.0, 50.0), cadcraft_engine::grips::GripMode::Stretch).unwrap();
    let doc = s.doc().unwrap();
    let ent = doc.entity(h).unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        assert_eq!(img.insert, Vec3::new(50.0, 50.0, 0.0));
    } else {
        panic!("expected image");
    }

    // 3. Stretch top-mid grip 5 -> rotates image around center
    let center = s.grips_of(h).unwrap()[4];
    // Rotate 90 degrees CCW: vector (0, 100) becomes (-100, 0)
    let new_top_mid = center + Vec2::new(-100.0, 0.0);
    s.grip_edit(h, 5, new_top_mid, cadcraft_engine::grips::GripMode::Stretch).unwrap();
    let doc = s.doc().unwrap();
    let ent = doc.entity(h).unwrap();
    if let EntityKind::Image(img) = &ent.kind {
        // u was (2, 0) rotated by 90 deg CCW becomes (0, 2)
        assert!(img.u.x.abs() < 1e-9);
        assert!((img.u.y - 2.0).abs() < 1e-9);
    } else {
        panic!("expected image");
    }

    // 4. Rotate mode around corner 0
    let corner0 = s.grips_of(h).unwrap()[0];
    s.grip_edit(h, 0, corner0 + Vec2::new(10.0, 0.0), cadcraft_engine::grips::GripMode::Rotate).unwrap();
    assert!(s.doc().unwrap().entity(h).is_some());
}

#[test]
fn find_image_file_resolves_drawing_relative_and_windows_paths() {
    use std::fs;
    let temp = std::env::temp_dir().join(format!("cadcraft_test_img_{}", std::process::id()));
    let sub = temp.join("images");
    let _ = fs::create_dir_all(&sub);

    let img1 = temp.join("site.png");
    let img2 = sub.join("logo.png");
    let _ = fs::write(&img1, b"fake_png_data");
    let _ = fs::write(&img2, b"fake_logo_data");

    let drawing_path = temp.join("project.dxf");
    let doc_str = drawing_path.to_str().unwrap();

    // 1. Same directory as drawing
    let found1 = cadcraft_engine::cmd::image::find_image_file("site.png", Some(doc_str));
    assert_eq!(found1, Some(img1.clone()));

    // 2. In subfolder "images" next to drawing
    let found2 = cadcraft_engine::cmd::image::find_image_file("images/logo.png", Some(doc_str));
    assert_eq!(found2, Some(img2.clone()));

    // 3. Windows-style backslashes "images\logo.png"
    let found3 = cadcraft_engine::cmd::image::find_image_file("images\\logo.png", Some(doc_str));
    assert_eq!(found3, Some(img2.clone()));

    // 4. Just filename "logo.png" automatically searched in "images/" subfolder
    let found4 = cadcraft_engine::cmd::image::find_image_file("logo.png", Some(doc_str));
    assert_eq!(found4, Some(img2.clone()));

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn saving_does_not_export_embedded_images_to_disk() {
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks { read: |_, _| Err("no reader".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });

    let temp_dir = std::env::temp_dir().join(format!("cadcraft_test_save_img_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dwg_path = temp_dir.join("test_drawing.dwg");

    let mut s = Session::new();
    let d = s.doc_mut().unwrap();

    // Insert an embedded OLE image with fake BMP bytes
    let fake_bmp = b"BM\x3a\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00fake_bmp_data".to_vec();
    d.store_image_asset("__ole_5A__", fake_bmp.clone());

    let img = cadcraft_doc::Image {
        insert: Vec3::ZERO,
        u: Vec3::new(10.0, 0.0, 0.0),
        v: Vec3::new(0.0, 10.0, 0.0),
        size: Vec2::new(100.0, 100.0),
        path: "__ole_5A__".to_string(),
        name: "OLE Picture".to_string(),
        ..cadcraft_doc::Image::default()
    };
    let h = d.new_handle();
    d.model.push(cadcraft_doc::Entity::new(h, cadcraft_doc::EntityKind::Image(img)));

    // Save as DWG
    let res = s.execute("saveas", &json!({ "path": dwg_path.to_str().unwrap() }));
    assert!(res.is_ok(), "Save should succeed: {:?}", res.err());

    // Verify no loose image files were exported to disk
    let loose_img_file = temp_dir.join("test_drawing_ole_5A.bmp");
    assert!(!loose_img_file.exists(), "Embedded image should remain in the file and NOT be exported to disk");

    // Verify the Image entity in the session still points to its embedded asset key
    let doc = s.doc().unwrap();
    let ent = doc.model.get(h).unwrap();
    if let cadcraft_doc::EntityKind::Image(saved_img) = &ent.kind {
        assert_eq!(saved_img.path, "__ole_5A__");
    } else {
        panic!("expected Image entity");
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
