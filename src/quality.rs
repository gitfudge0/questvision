use crate::config::Config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Performance,
    Balanced,
    Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamSettings {
    pub preset: Preset,
    pub fps: u32,
    pub bitrate_mbps: u32,
}

impl StreamSettings {
    pub fn from_offer(
        config: &Config,
        quality: Option<&str>,
        fps: Option<u32>,
        bitrate_mbps: Option<u32>,
    ) -> Result<Self, &'static str> {
        let preset = match quality.unwrap_or("balanced") {
            "performance" => Preset::Performance,
            "balanced" => Preset::Balanced,
            "quality" => Preset::Quality,
            _ => return Err("unknown quality preset"),
        };
        let fps = fps.unwrap_or(config.fps);
        let bitrate_mbps = bitrate_mbps.unwrap_or(match preset {
            Preset::Performance => 10,
            Preset::Balanced => 20,
            Preset::Quality => 35,
        });
        if !(15..=120).contains(&fps) {
            return Err("FPS must be between 15 and 120");
        }
        if !(2..=80).contains(&bitrate_mbps) {
            return Err("bitrate must be between 2 and 80 Mbps");
        }
        Ok(Self {
            preset,
            fps,
            bitrate_mbps,
        })
    }
    pub fn output_size(self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width < 2 || height < 2 {
            return None;
        }
        let limit = match self.preset {
            Preset::Performance => Some((1280, 720)),
            Preset::Balanced => Some((1920, 1080)),
            Preset::Quality => None,
        };
        let (w, h) = match limit {
            Some((mw, mh)) if width > mw || height > mh => {
                if u64::from(width) * u64::from(mh) >= u64::from(height) * u64::from(mw) {
                    (
                        mw,
                        (u64::from(height) * u64::from(mw) / u64::from(width)) as u32,
                    )
                } else {
                    (
                        (u64::from(width) * u64::from(mh) / u64::from(height)) as u32,
                        mh,
                    )
                }
            }
            _ => (width, height),
        };
        let even_w = w & !1;
        let even_h = h & !1;
        (even_w >= 2 && even_h >= 2).then_some((even_w, even_h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aspect_and_even_dimensions() {
        let c = Config::default();
        let perf = StreamSettings::from_offer(&c, Some("performance"), None, None).unwrap();
        assert_eq!(perf.output_size(2880, 1800), Some((1152, 720)));
        assert_eq!(perf.output_size(1920, 1080), Some((1280, 720)));
        assert_eq!(perf.output_size(1279, 719), Some((1278, 718)));
        let balanced = StreamSettings::from_offer(&c, Some("balanced"), None, None).unwrap();
        assert_eq!(balanced.output_size(2880, 1800), Some((1728, 1080)));
        let quality = StreamSettings::from_offer(&c, Some("quality"), None, None).unwrap();
        assert_eq!(quality.output_size(2880, 1800), Some((2880, 1800)));
    }
    #[test]
    fn validates_controls_and_defaults() {
        let c = Config::default();
        assert_eq!(
            StreamSettings::from_offer(&c, Some("performance"), None, None)
                .unwrap()
                .bitrate_mbps,
            10
        );
        assert_eq!(
            StreamSettings::from_offer(&c, Some("balanced"), Some(90), Some(50))
                .unwrap()
                .fps,
            90
        );
        assert!(StreamSettings::from_offer(&c, Some("bad"), None, None).is_err());
        assert!(StreamSettings::from_offer(&c, None, Some(0), None).is_err());
        assert!(StreamSettings::from_offer(&c, None, None, Some(100)).is_err());
    }
}
