use bdauthor_core::audio::{AudioFormat, AudioTrackCfg, AC3_RATES};
use bdauthor_core::ffmpeg::audio_codec_args;
use bdauthor_core::json;

fn cfg(fmt: AudioFormat, br: u32) -> AudioTrackCfg { let mut t = AudioTrackCfg::embedded(0, "eng"); t.format = fmt; t.bitrate = br; t }

#[test]
fn format_and_bitrate_round_trip_in_json() {
    for (f, br, name) in [(AudioFormat::Ac3Surround, 0, "ac3_51"), (AudioFormat::Ac3Copy, 0, "ac3_copy"), (AudioFormat::Ac3, 256, "ac3")] {
        let t = cfg(f, br);
        let v = t.to_value();
        let back = AudioTrackCfg::from_value(&json::parse(&v.to_pretty()).unwrap()).unwrap();
        assert_eq!(back, t);
        assert_eq!(f.name(), name);
    }
    // un debit nul et le format AC-3 ne sont pas ecrits : les anciens projets restent identiques
    let s = cfg(AudioFormat::Ac3, 0).to_value().to_pretty();
    assert!(!s.contains("format") && !s.contains("bitrate"), "{s}");
}

#[test]
fn bitrate_is_checked() {
    assert!(cfg(AudioFormat::Ac3, 256).problems().is_empty());
    assert!(!cfg(AudioFormat::Ac3, 250).problems().is_empty());
    assert!(!cfg(AudioFormat::Lpcm24, 256).problems().is_empty());
    assert!(!cfg(AudioFormat::Ac3Copy, 256).problems().is_empty());
    assert!(AC3_RATES.contains(&448) && AC3_RATES.contains(&640));
}

#[test]
fn track_bitrates() {
    assert_eq!(cfg(AudioFormat::Ac3, 0).kbps(6, 0, 192), 192);
    assert_eq!(cfg(AudioFormat::Ac3, 128).kbps(6, 0, 192), 128);
    assert_eq!(cfg(AudioFormat::Ac3Surround, 0).kbps(6, 0, 192), 448);
    assert_eq!(cfg(AudioFormat::Ac3Surround, 640).kbps(6, 0, 192), 640);
    assert_eq!(cfg(AudioFormat::Ac3Copy, 0).kbps(6, 384, 192), 384);
    assert_eq!(cfg(AudioFormat::Ac3Copy, 0).kbps(6, 0, 192), 640);
    assert_eq!(cfg(AudioFormat::Lpcm16, 0).kbps(6, 0, 192), 6 * 16 * 48);
}

#[test]
fn ffmpeg_arguments_per_track() {
    // sans reglage particulier : les memes options qu'avant pour toutes les pistes
    let plain = audio_codec_args("192k", &[cfg(AudioFormat::Ac3, 0), cfg(AudioFormat::Ac3, 0)]).join(" ");
    assert_eq!(plain, "-c:a ac3 -b:a 192k -ar 48000 -ac 2");
    let a = audio_codec_args("192k", &[cfg(AudioFormat::Ac3Surround, 0), cfg(AudioFormat::Ac3Copy, 0), cfg(AudioFormat::Ac3, 128), cfg(AudioFormat::Ac3, 0)]).join(" ");
    assert_eq!(a, "-ar 48000 -c:a:0 ac3 -b:a:0 448k -ac:a:0 6 -c:a:1 copy -c:a:2 ac3 -b:a:2 128k -ac:a:2 2 -c:a:3 ac3 -b:a:3 192k -ac:a:3 2");
}
