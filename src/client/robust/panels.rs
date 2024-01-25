/// Dimensions of a panel.
#[derive(Debug, Default, Clone, Copy)]
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
    ) -> Option<((u16, u16), (u16, u16))> {

        debug_assert!(weight >= 0.0 && weight <= 1.0,
            "weight must be between 0 and 1, got {weight}"
        );

        let total_padding = left_padding + middle_padding + right_padding;

        if total_padding > segment.1 {
            return None;
        }

        let true_size = segment.1 - total_padding;

        // Get the size of the two segments
        let sizes = Self::split(true_size, weight);

        // Get the positions
        let posns = (
            segment.0 + left_padding,
            segment.0 + left_padding + sizes.0 + middle_padding
        );

        Some(((posns.0, sizes.0), (posns.1, sizes.1)))
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
        -> Option<(Self, Self, Self)>
    {
        if off_left + off_right > self.size.0 { return None; };
        Some((
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
        let (top, bottom) = Self::split(self.size.0, weight);

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
        -> Option<(Self, Self, Self)>
    {
        if off_top + off_bottom > self.size.1 { return None; };
        Some((
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
    ) -> Option<(Self, Self)> {
        let (left, right) = self.split_horiz(weight);
        let (_, left, _) = left.shave_horiz(left_padding, middle_padding)?;
        let (_, right, _) = right.shave_horiz(middle_padding, right_padding)?;
        Some((left, right))
    }
}