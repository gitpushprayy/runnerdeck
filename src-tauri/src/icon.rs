//! Tray icon: the RunnerDeck stack, composed at runtime from the sprites in icons/tray/
//! (drawn by scripts/draw-tray.ps1). One layer per runner (GitHub 1, GitHub 2, TeamCity, top to
//! bottom): a layer is lit while that runner is busy and dimmed while it idles. A badge in the
//! corner gives the overall state; while anything runs it pulses blue.

use std::sync::OnceLock;
use tauri::image::Image;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    /// all runners idle and connected
    Idle,
    /// at least one runner is running a job or build
    Busy,
    /// paused, a runner stopped, or disk above the prune target
    Warn,
    /// dashboard service unreachable, or disk over the hard cap
    Down,
}

const SIZE: u32 = 32;

struct Sprites {
    lit: [Vec<u8>; 3],
    dim: [Vec<u8>; 3],
    idle: Vec<u8>,
    busy: Vec<u8>,
    busy_pulse: Vec<u8>,
    warn: Vec<u8>,
    down: Vec<u8>,
}

fn decode(bytes: &[u8]) -> Vec<u8> {
    let img = Image::from_bytes(bytes).expect("embedded tray sprite is a valid PNG");
    assert_eq!((img.width(), img.height()), (SIZE, SIZE), "tray sprites are 32x32");
    img.rgba().to_vec()
}

fn sprites() -> &'static Sprites {
    static S: OnceLock<Sprites> = OnceLock::new();
    S.get_or_init(|| Sprites {
        lit: [
            decode(include_bytes!("../icons/tray/layer-1-lit.png")),
            decode(include_bytes!("../icons/tray/layer-2-lit.png")),
            decode(include_bytes!("../icons/tray/layer-3-lit.png")),
        ],
        dim: [
            decode(include_bytes!("../icons/tray/layer-1-dim.png")),
            decode(include_bytes!("../icons/tray/layer-2-dim.png")),
            decode(include_bytes!("../icons/tray/layer-3-dim.png")),
        ],
        idle: decode(include_bytes!("../icons/tray/badge-idle.png")),
        busy: decode(include_bytes!("../icons/tray/badge-busy.png")),
        busy_pulse: decode(include_bytes!("../icons/tray/badge-busy-pulse.png")),
        warn: decode(include_bytes!("../icons/tray/badge-warn.png")),
        down: decode(include_bytes!("../icons/tray/badge-down.png")),
    })
}

/// Porter-Duff "source over" for straight (non-premultiplied) RGBA.
fn over(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = s[3] as f32 / 255.0;
        if sa == 0.0 {
            continue;
        }
        let da = d[3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        for c in 0..3 {
            let v = (s[c] as f32 * sa + d[c] as f32 * da * (1.0 - sa)) / oa;
            d[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        d[3] = (oa * 255.0).round() as u8;
    }
}

/// `lit[i]`: layer i (0 = GitHub 1 at the top) is lit. `pulse`: the alternate frame of the busy badge.
pub fn tray_icon(health: Health, lit: [bool; 3], pulse: bool) -> Image<'static> {
    let s = sprites();
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    for i in (0..3).rev() {
        // bottom layer first
        over(&mut px, if lit[i] { &s.lit[i] } else { &s.dim[i] });
    }
    let badge = match health {
        Health::Idle => &s.idle,
        Health::Busy if pulse => &s.busy_pulse,
        Health::Busy => &s.busy,
        Health::Warn => &s.warn,
        Health::Down => &s.down,
    };
    over(&mut px, badge);
    Image::new_owned(px, SIZE, SIZE)
}
