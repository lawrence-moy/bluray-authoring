use bdauthor_core::json::*;
use bdauthor_core::palette::RgbaImage;
use bdauthor_core::png::write_png;

#[test]
fn json_basic() {
    let v = parse(r#"{"title":"Mes vidéos 😀","n":[1,2.5,-3e2,{"t":95,"title":"Le pont"}],"ok":true,"x":null,"n":7}"#).unwrap();
    assert_eq!(v.get("title").unwrap().as_str().unwrap(), "Mes vidéos 😀");
    assert_eq!(v.get("n").unwrap().as_i64(), Some(7), "derniere valeur gagne");
    assert_eq!(v.get("ok").unwrap().as_bool(), Some(true));
    assert!(v.get("x").unwrap().is_null());
    let v = parse("\u{feff}[1, 2.5, -300, {\"t\": 95}]").unwrap();
    assert_eq!(v.as_array().unwrap()[2].as_f64(), Some(-300.0));
}

#[test]
fn json_errors() {
    for bad in ["", "{", "[1,]", "{\"a\" 1}", "[1] x", "\"abc", "{'a':1}", "[01x]", "tru"] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
    assert!(parse("[1,]").unwrap_err().to_string().contains("ligne 1"));
}

#[test]
fn png_decodes_with_ffprobe() {
    let mut im = RgbaImage::new(70, 40);
    for y in 0..40 { for x in 0..70 { im.set(x, y, [(x * 3) as u8, (y * 5) as u8, 90, 255]); } }
    let p = std::env::temp_dir().join(format!("bdauthor_png_test_{}.png", std::process::id()));
    std::fs::write(&p, write_png(&im)).unwrap();
    if let Ok(out) = std::process::Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=width,height,codec_name", "-of", "csv=p=0"]).arg(&p).output() {
        if out.status.success() {
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "png,70,40");
        } else {
            panic!("ffprobe refuse le PNG : {}", String::from_utf8_lossy(&out.stderr));
        }
    }
    let _ = std::fs::remove_file(p);
}
