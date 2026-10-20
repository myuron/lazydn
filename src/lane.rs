use crate::model::{Lane, Rgb};

const PALETTE: [Rgb; 6] = [
    Rgb(0xff, 0x8a, 0x4c),
    Rgb(0x51, 0xcf, 0x66),
    Rgb(0xda, 0x77, 0xf2),
    Rgb(0x3b, 0xc9, 0xdb),
    Rgb(0xf0, 0x65, 0x95),
    Rgb(0xfc, 0xc4, 0x19),
];

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

pub fn rgb(lane: Lane) -> Rgb {
    match lane {
        Lane::Index(i) => PALETTE[usize::from(i).min(5)],
        Lane::Custom(rgb) => rgb,
    }
}

pub fn auto_lane(id: &str) -> u8 {
    let h = id
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    (h % 6) as u8
}

pub fn to_ansi256(rgb: Rgb) -> u8 {
    let Rgb(r, g, b) = rgb;
    let nearest = |v: u8| {
        (0..6)
            .min_by_key(|i| (i32::from(v) - i32::from(CUBE_LEVELS[*i])).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = Rgb(CUBE_LEVELS[ri], CUBE_LEVELS[gi], CUBE_LEVELS[bi]);
    let cube_index = 16 + 36 * ri + 6 * gi + bi;

    let avg = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    let step = ((avg - 8 + 5) / 10).clamp(0, 23);
    let level = (8 + 10 * step) as u8;
    let gray = Rgb(level, level, level);

    if distance(rgb, gray) < distance(rgb, cube) {
        (232 + step) as u8
    } else {
        cube_index as u8
    }
}

fn distance(a: Rgb, b: Rgb) -> i32 {
    let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 14. lane

    #[test]
    fn t14_index_lanes_map_to_the_spec_palette() {
        let expected = [
            Rgb(0xff, 0x8a, 0x4c),
            Rgb(0x51, 0xcf, 0x66),
            Rgb(0xda, 0x77, 0xf2),
            Rgb(0x3b, 0xc9, 0xdb),
            Rgb(0xf0, 0x65, 0x95),
            Rgb(0xfc, 0xc4, 0x19),
        ];
        for (i, want) in expected.into_iter().enumerate() {
            assert_eq!(rgb(Lane::Index(i as u8)), want, "lane {i}");
        }
    }

    #[test]
    fn t14_custom_lane_maps_to_its_own_rgb() {
        assert_eq!(rgb(Lane::Custom(Rgb(1, 2, 3))), Rgb(1, 2, 3));
    }

    #[test]
    fn t14_auto_lane_matches_server_hash_of_id() {
        assert_eq!(auto_lane("01a13fa8-6cd4-70ef-b8e5-f46a3ca70ca8"), 1);
        assert_eq!(auto_lane("a"), 1);
        assert_eq!(auto_lane(""), 0);
        assert_eq!(auto_lane("日本語"), 5);
        assert_eq!(auto_lane("😀"), 4);
    }

    #[test]
    fn t14_auto_lane_is_in_range_and_deterministic() {
        for name in [
            "01a13fa8-6cd4-70ef-b8e5-f46a3ca70ca8",
            "01a15000-0000-7000-8000-0000000b9b9b",
            "01a15764-e962-7084-b8a9-b146b4c5f3c4",
        ] {
            let lane = auto_lane(name);
            assert!(lane < 6, "{name}: {lane}");
            assert_eq!(lane, auto_lane(name));
        }
    }

    // 15. 256 è²

    #[test]
    fn t15_black_and_white_map_to_cube_corners() {
        assert_eq!(to_ansi256(Rgb(0x00, 0x00, 0x00)), 16);
        assert_eq!(to_ansi256(Rgb(0xff, 0xff, 0xff)), 231);
    }

    #[test]
    fn t15_orange_maps_to_nearest_cube_color() {
        assert_eq!(to_ansi256(Rgb(0xff, 0x8a, 0x4c)), 209);
    }

    #[test]
    fn t15_mid_gray_maps_into_grayscale_ramp() {
        assert_eq!(to_ansi256(Rgb(0x80, 0x80, 0x80)), 244);
    }
}
