//! Encodeurs materiels : options ffmpeg, liste des encodeurs, classement des echecs, memorisation, et detection reelle (sans
//! carte graphique ici : les encodeurs presents doivent etre « non disponibles », pas refuses pour une option inconnue).
use bdauthor_core::ffmpeg::{self, EncodeOpts, Scan, Tools};
use bdauthor_core::hwenc::*;
use std::sync::atomic::AtomicBool;

#[test]
fn encoder_names_and_options() {
    for e in [VideoEncoder::X264, VideoEncoder::Qsv, VideoEncoder::Nvenc, VideoEncoder::Amf] {
        assert_eq!(VideoEncoder::from_name(e.name()), Some(e));
    }
    let q = hw_args(VideoEncoder::Qsv, 18, None, 30000, 24, "slow", Scan::Progressive, 1080).join(" ");
    assert!(q.starts_with("-c:v h264_qsv -profile:v high -preset slow -level 41 -b:v 9000k -maxrate 30000k -bufsize 30000k -g 24 -bf 3 -refs 4"), "{q}");
    assert!(q.ends_with("-flags +cgop"), "{q}");
    let n = hw_args(VideoEncoder::Nvenc, 24, Some(12000), 30000, 25, "medium", Scan::PsF, 1080).join(" ");
    assert!(n.contains("-preset p5") && n.contains("-b:v 12000k") && n.contains("-bluray-compat 1") && n.ends_with("-flags +cgop+ildct+ilme -field_order tt"), "{n}");
    let a = hw_args(VideoEncoder::Amf, 18, None, 30000, 24, "veryslow", Scan::Bff, 720).join(" ");
    assert!(a.contains("-quality quality") && a.contains("-enforce_hrd 1") && a.contains("-b:v 4000k") && a.ends_with("-field_order bb"), "{a}");
    // ou le materiel s'applique : progressif ; entrelace et PsF seulement s'il code l'entrelace ; jamais en deux passes
    assert!(applies(VideoEncoder::Qsv, false, Scan::Progressive, false));
    assert!(!applies(VideoEncoder::Qsv, false, Scan::PsF, false) && applies(VideoEncoder::Qsv, true, Scan::PsF, false));
    assert!(!applies(VideoEncoder::Qsv, true, Scan::Progressive, true) && !applies(VideoEncoder::X264, true, Scan::Progressive, false));
}

#[test]
fn encode_args_use_the_hardware_encoder_when_it_applies() {
    let info = ffmpeg::VideoInfo { width: 1920, height: 1080, fps_num: 24000, fps_den: 1001, duration_s: 10.0, codec: "h264".into(), ..Default::default() };
    let mut o = EncodeOpts { encoder: VideoEncoder::Qsv, ..Default::default() };
    let (a, tg) = ffmpeg::encode_args(std::path::Path::new("in.mkv"), std::path::Path::new("out.ts"), &info, &o);
    let j = a.join(" ");
    assert!(o.uses_hw(tg.scan) && j.contains("-c:v h264_qsv") && j.contains("format=nv12") && !j.contains("libx264"), "{j}");
    // remplir le disque (deux passes) : x264
    o.video_kbps = Some(20000);
    let j = ffmpeg::encode_args(std::path::Path::new("in.mkv"), std::path::Path::new("out.ts"), &info, &o).0.join(" ");
    assert!(j.contains("libx264") && j.contains("format=yuv420p"), "{j}");
}

#[test]
fn encoder_list_and_failures_are_read() {
    let list = " V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)\n V..... h264_qsv             H.264 (Intel Quick Sync Video acceleration) (codec h264)\n V....D libx264              libx264 H.264";
    assert_eq!(listed_encoders(list), vec![VideoEncoder::Qsv, VideoEncoder::Nvenc]);
    // vrais messages de ffmpeg sans carte graphique
    let nv = "[h264_nvenc @ 0x5592] Cannot load libcuda.so.1\n[vost#0:0/h264_nvenc @ 0x55] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height.\nError while filtering: Operation not permitted\nConversion failed!";
    assert_eq!(classify_failure(nv), Status::Unavailable("Cannot load libcuda.so.1".into()));
    let qsv = "[h264_qsv @ 0x55d8] Error creating a MFX session: -9.\nConversion failed!";
    assert!(matches!(classify_failure(qsv), Status::Unavailable(m) if m.contains("MFX session")));
    let lvl = "[h264_nvenc @ 0x1] InitializeEncoder failed: invalid param (8): Invalid Level.\nConversion failed!";
    assert!(matches!(classify_failure(lvl), Status::Refused(m) if m.contains("Invalid Level")));
    assert!(matches!(classify_failure("Unrecognized option 'tune'.\nError splitting the argument list: Option not found"), Status::Refused(_)));
}

#[test]
fn detection_is_saved_in_one_line() {
    let d = Detection {
        probes: vec![
            Probe { encoder: VideoEncoder::Qsv, status: Status::Ok, fps: 121.0, interlaced: Some(Status::Ok) },
            Probe { encoder: VideoEncoder::Nvenc, status: Status::Unavailable("x".into()), fps: 0.0, interlaced: None },
            Probe { encoder: VideoEncoder::Amf, status: Status::NotInFfmpeg, fps: 0.0, interlaced: None },
        ],
        x264_fps: 8.5,
    };
    let s = d.to_config();
    assert_eq!(s, "qsv=ok,121.0,i-ok;nvenc=unavailable,0.0;amf=absent,0.0;x264=8.5");
    let back = Detection::from_config(&s).unwrap();
    assert!(back.probe(VideoEncoder::Qsv).unwrap().usable() && back.probe(VideoEncoder::Qsv).unwrap().interlaced_ok());
    assert!(!back.probe(VideoEncoder::Nvenc).unwrap().usable());
    assert_eq!(back.x264_fps, 8.5);
}

#[test]
fn real_detection_reports_missing_hardware_cleanly() {
    let Ok(tools) = Tools::locate() else { eprintln!("ffmpeg absent : test ignore"); return };
    let mut steps = Vec::new();
    let d = detect(&tools, &mut |f, s| steps.push((f, s.to_string())), &AtomicBool::new(false)).unwrap();
    eprintln!("{d:#?}");
    assert_eq!(d.probes.len(), 3);
    assert!(d.x264_fps > 0.0);
    assert!(steps.windows(2).all(|w| w[1].0 >= w[0].0) && steps.last().unwrap().0 == 1.0);
    for p in &d.probes {
        // ici, pas de carte : jamais « refuse » pour une option que ce ffmpeg ne connaitrait pas
        if let Status::Refused(m) = &p.status { assert!(!m.contains("Option not found") && !m.contains("Unrecognized"), "{m}"); }
    }
}
