//! Timecode « hh:mm:ss:ii » (ii = numero de l'image dans la seconde) : affichage et saisie des instants d'une video.

/// Cadence utilisee quand celle de la video n'est pas connue.
pub const DEFAULT_FPS: f64 = 25.0;

fn fps_or_default(fps: f64) -> f64 { if fps.is_finite() && fps >= 1.0 { fps } else { DEFAULT_FPS } }

/// Instant en secondes -> « hh:mm:ss:ii ». Le temps est reel (pas de timecode « drop frame ») : l'image est le rang dans la
/// seconde (0 a cadence nominale - 1, ex. 0 a 23 pour 23,976 i/s).
pub fn format(t: f64, fps: f64) -> String {
    let fps = fps_or_default(fps);
    let nominal = fps.round().max(1.0) as i64;
    let t = if t.is_finite() { t.max(0.0) } else { 0.0 };
    let mut s = t.floor() as i64;
    let mut f = ((t - s as f64) * fps).round() as i64;
    if f >= nominal { s += 1; f = 0; }
    format!("{:02}:{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60, f)
}

/// Saisie -> instant en secondes. Accepte « hh:mm:ss:ii », « mm:ss:ii », « ss:ii » (separateurs « : », « ; » ou « . ») ou un
/// nombre de secondes seul (« 90 », « 90.5 »).
pub fn parse(s: &str, fps: f64) -> Option<f64> {
    let fps = fps_or_default(fps);
    let s = s.trim();
    if s.is_empty() { return None; }
    if !s.contains([':', ';']) {
        // un seul nombre : des secondes ; avec deux points ou plus (« 0.1.5 ») on retombe sur le timecode
        if let Ok(v) = s.replace(',', ".").parse::<f64>() { return v.is_finite().then_some(v.max(0.0)); }
    }
    let parts: Vec<&str> = s.split([':', ';', '.']).map(str::trim).collect();
    if parts.len() < 2 || parts.len() > 4 { return None; }
    let nums: Option<Vec<u64>> = parts.iter().map(|p| p.parse::<u64>().ok()).collect();
    let nums = nums?;
    // alignes a droite : images, secondes, minutes, heures
    let mut it = nums.iter().rev();
    let f = *it.next()? as f64;
    let sec = *it.next()? as f64;
    let min = it.next().copied().unwrap_or(0) as f64;
    let h = it.next().copied().unwrap_or(0) as f64;
    Some(h * 3600.0 + min * 60.0 + sec + f / fps)
}
