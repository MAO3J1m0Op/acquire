use std::fmt;
use thiserror::Error;

/// Dimensions of a panel.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PanelDim {
    /// The position of the top-left corner of the panel.
    pub top_left: (u16, u16),
    /// Size of the panel.
    pub size: (u16, u16),
}

impl PanelDim {
    /// Divides an integer in two based on the supplied weight. The weight, a
    /// quantity between 0 and 1, indicates the proportion of the original
    /// segment that will be dedicated to the second segment, with 0.5
    /// indicating an even split, 0 indicating the first number receives it all, and
    /// 1 indicating the second segment receives it all.
    #[inline]
    fn split(num: u16, weight: f64) -> (u16, u16) {
        let left = num as f64 * weight;
        let left = if weight <= 0.5 {
            left.ceil() as u16
        } else {
            left.floor() as u16
        };
        (left, num - left)
    }

    /// Splits a line segment with position (`.0`) and size (`.1`) into two
    /// segments, removing some of the segment in between for padding.
    fn split_with_padding(
        segment: (u16, u16),
        weight: f64,
        left_padding: u16,
        middle_padding: u16,
        right_padding: u16
    ) -> Result<((u16, u16), (u16, u16)), PanelTooSmallError> {

        debug_assert!(weight >= 0.0 && weight <= 1.0,
            "weight must be between 0 and 1, got {weight}"
        );

        let total_padding = left_padding + middle_padding + right_padding;

        if total_padding > segment.1 {
            return Err(PanelTooSmallError);
        }

        let true_size = segment.1 - total_padding;

        // Get the size of the two segments
        let sizes = Self::split(true_size, weight);

        // Get the positions
        let posns = (
            segment.0 + left_padding,
            segment.0 + left_padding + sizes.0 + middle_padding
        );

        Ok(((posns.0, sizes.0), (posns.1, sizes.1)))
    }

    pub fn split_horiz(self, weight: f64) -> (Self, Self) {
        let (left, right) = Self::split(self.size.0, weight);

        (
            Self {
                top_left: self.top_left,
                size: (left, self.size.1),
            },
            Self {
                top_left: (self.top_left.0 + left, self.top_left.1),
                size: (right, self.size.1),
            }
        )
    }

    pub fn area(&self) -> u64 {
        self.size.0 as u64 * self.size.1 as u64
    }

    pub fn shave_horiz(self, off_left: u16, off_right: u16)
        -> Result<(Self, Self, Self), PanelTooSmallError>
    {
        if off_left + off_right > self.size.0 { return Err(PanelTooSmallError); };
        Ok((
            Self {
                top_left: self.top_left,
                size: (off_left, self.size.1),
            },
            Self {
                top_left: (self.top_left.0 + off_left, self.top_left.1),
                size: (self.size.0 - off_left - off_right, self.size.1)
            },
            Self {
                top_left: (self.top_left.0 + self.size.0 - off_right, self.top_left.1),
                size: (off_right, self.size.1)
            }
        ))
    }

    pub fn split_vert(self, weight: f64) -> (Self, Self) {
        let (top, bottom) = Self::split(self.size.1, weight);

        (
            Self {
                top_left: self.top_left,
                size: (self.size.0, top),
            },
            Self {
                top_left: (self.top_left.0, self.top_left.1 + top),
                size: (self.size.0, bottom),
            }
        )
    }

    pub fn shave_vert(self, off_top: u16, off_bottom: u16)
        -> Result<(Self, Self, Self), PanelTooSmallError>
    {
        if off_top + off_bottom > self.size.1 { return Err(PanelTooSmallError); };
        Ok((
            Self {
                top_left: self.top_left,
                size: (self.size.0, off_top),
            },
            Self {
                top_left: (self.top_left.0, self.top_left.1 + off_top),
                size: (self.size.0, self.size.1 - off_top - off_bottom)
            },
            Self {
                top_left: (self.top_left.0, self.top_left.1 + self.size.1 - off_bottom),
                size: (self.size.0, off_bottom)
            }
        ))
    }

    pub fn split_pad_horiz(self,
        weight: f64,
        left_padding: u16,
        middle_padding: u16,
        right_padding: u16
    ) -> Result<(Self, Self), PanelTooSmallError> {
        let (left, right) = self.split_horiz(weight);
        let (_, left, _) = left.shave_horiz(left_padding, middle_padding)?;
        let (_, right, _) = right.shave_horiz(middle_padding, right_padding)?;
        Ok((left, right))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub struct PanelTooSmallError;

impl fmt::Display for PanelTooSmallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "panel too small for desired operation")
    }
}

#[cfg(test)]
mod test {
    use crate::client::robust::panels::PanelTooSmallError;

    use super::PanelDim;

    #[test]
    fn even_split() {
        let panel = PanelDim {
            top_left: (2, 3),
            size: (50, 50),
        };

        let (left, right) = panel.split_horiz(0.5);

        assert_eq!(left, PanelDim {
            top_left: (2, 3),
            size: (25, 50),
        });
        assert_eq!(right, PanelDim {
            top_left: (27, 3),
            size: (25, 50),
        });

        let (top, bottom) = panel.split_vert(0.5);

        assert_eq!(top, PanelDim {
            top_left: (2, 3),
            size: (50, 25),
        });
        assert_eq!(bottom, PanelDim {
            top_left: (2, 28),
            size: (50, 25),
        })
    }

    #[test]
    fn uneven_split() {
        let panel = PanelDim {
            top_left: (2, 3),
            size: (30, 30),
        };

        let one_third: f64 = 1.0 / 3.0;

        let (left, right) = panel.split_horiz(one_third);

        assert_eq!(left, PanelDim {
            top_left: (2, 3),
            size: (10, 30),
        });
        assert_eq!(right, PanelDim {
            top_left: (12, 3),
            size: (20, 30),
        });

        let (top, bottom) = panel.split_vert(one_third);

        assert_eq!(top, PanelDim {
            top_left: (2, 3),
            size: (30, 10),
        });
        assert_eq!(bottom, PanelDim {
            top_left: (2, 13),
            size: (30, 20),
        })
    }

    #[test]
    fn uneven_split_rounding() {
        let panel = PanelDim {
            top_left: (2, 3),
            size: (50, 40),
        };

        let one_third: f64 = 1.0 / 3.0;

        let (left, right) = panel.split_horiz(one_third);

        assert!(left.size.0 + right.size.0 == panel.size.0);
        assert!(left.size.1 + right.size.1 == 2 * panel.size.1);

        let (top, bottom) = panel.split_vert(one_third);

        assert!(top.size.0 + bottom.size.0 == 2 * panel.size.0);
        dbg!(top, bottom, panel);
        assert!(top.size.1 + bottom.size.1 == panel.size.1);
    }

    #[test]
    fn shave() {
        let panel = PanelDim {
            top_left: (2, 3),
            size: (30, 30),
        };

        let (left, mid, right) = panel.shave_horiz(10, 10).unwrap();

        assert_eq!(left, PanelDim {
            top_left: (2, 3),
            size: (10, 30),
        });
        assert_eq!(mid, PanelDim {
            top_left: (12, 3),
            size: (10, 30),
        });
        assert_eq!(right, PanelDim {
            top_left: (22, 3),
            size: (10, 30),
        });

        let (top, mid, bot) = panel.shave_vert(10, 10).unwrap();

        assert_eq!(top, PanelDim {
            top_left: (2, 3),
            size: (30, 10),
        });
        assert_eq!(mid, PanelDim {
            top_left: (2, 13),
            size: (30, 10),
        });
        assert_eq!(bot, PanelDim {
            top_left: (2, 23),
            size: (30, 10),
        });
    }

    #[test]
    fn shave_fail() {
        let panel = PanelDim {
            top_left: (2, 3),
            size: (10, 10),
        };

        let result = panel.shave_horiz(15, 0);
        assert_eq!(result.unwrap_err(), PanelTooSmallError);
        let result = panel.shave_horiz(0, 15);
        assert_eq!(result.unwrap_err(), PanelTooSmallError);
        let result = panel.shave_vert(15, 0);
        assert_eq!(result.unwrap_err(), PanelTooSmallError);
        let result = panel.shave_vert(0, 15);
        assert_eq!(result.unwrap_err(), PanelTooSmallError);
    }
}
