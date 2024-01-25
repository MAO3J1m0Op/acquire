use std::{ops, fmt, str::FromStr};

use serde::ser::SerializeStruct;

use serde::{Serialize, Deserialize, Deserializer};
use termion::color::{Color, self};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Company {
    Continental,
    Imperial,
    American,
    Festival,
    Worldwide,
    Luxor,
    Tower,
}

impl Company {
    pub fn id(&self) -> usize {
        match self {
            Continental => 0,
            Imperial => 1,
            American => 2,
            Festival => 3,
            Worldwide => 4,
            Luxor => 5,
            Tower => 6,
        }
    }

    fn inv_id(val: u8) -> Company {
        match val {
            0 => Continental,
            1 => Imperial,
            2 => American,
            3 => Festival,
            4 => Worldwide,
            5 => Luxor,
            6 => Tower,
            _ => panic!("No corresponding value"),
        }
    }
}

impl Color for Company {
    fn write_fg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Continental => color::Cyan.write_fg(f),
            Imperial => color::Magenta.write_fg(f),
            American => color::Blue.write_fg(f),
            Festival => color::Green.write_fg(f),
            Worldwide => color::White.write_fg(f),
            Luxor => color::Red.write_fg(f),
            Tower => color::Yellow.write_fg(f),
        }
    }

    fn write_bg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Continental => color::Cyan.write_bg(f),
            Imperial => color::Magenta.write_bg(f),
            American => color::Blue.write_bg(f),
            Festival => color::Green.write_bg(f),
            Worldwide => color::White.write_bg(f),
            Luxor => color::Red.write_bg(f),
            Tower => color::Yellow.write_bg(f),
        }
    }
}

impl NiceFgColor for Company {
    fn write_nice_fg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Continental => color::Cyan.write_nice_fg(f),
            Imperial => color::Magenta.write_nice_fg(f),
            American => color::Blue.write_nice_fg(f),
            Festival => color::Green.write_nice_fg(f),
            Worldwide => color::White.write_nice_fg(f),
            Luxor => color::Red.write_nice_fg(f),
            Tower => color::Yellow.write_nice_fg(f),
        }
    }
}

use Company::*;

impl Company {
    /// Gets the color associated with each company.
    pub fn color(&self) -> &'static dyn NiceFgColor {
        use termion::color::*;

        match self {
            Continental => &Cyan,
            Imperial => &Magenta,
            American => &Blue,
            Festival => &Green,
            Worldwide => &White,
            Luxor => &Red,
            Tower => &Yellow,
        }
    }

    /// The first letter of the company for display use on the board.
    pub fn char(&self) -> char {
        match self {
            Continental => 'C',
            Imperial => 'I',
            American => 'A',
            Festival => 'F',
            Worldwide => 'W',
            Luxor => 'L',
            Tower => 'T',
        }
    }
}

impl fmt::Display for Company {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let str = match self {
            Continental => "Continental",
            Imperial => "Imperial",
            American => "American",
            Festival => "Festival",
            Worldwide => "Worldwide",
            Luxor => "Luxor",
            Tower => "Tower",
        };

        write!(f, "{}", str)
    }
}

impl FromStr for Company {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match &*s.to_ascii_lowercase() {
            "c" | "continental" => Ok(Continental),
            "i" | "imperial" => Ok(Imperial),
            "a" | "american" => Ok(American),
            "f" | "festival" => Ok(Festival),
            "w" | "worldwide" => Ok(Worldwide),
            "l" | "luxor" => Ok(Luxor),
            "t" | "tower" => Ok(Tower),
            _ => Err(())
        }
    }
}

/// Maps each possibility of the [`Company`] enum to a value.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompanyMap<T> {
    map: [T; 7],
}

impl<T> CompanyMap<T> {

    pub fn iter(&self) -> impl Iterator<Item = (Company, &T)> {
        self.into_iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Company, &mut T)> {
        self.into_iter()
    }

    /// Converts this `CompanyMap` into one of a different type using a mutation
    /// function.
    pub fn map<U, F: FnMut(Company, T) -> U>(self, mut f: F) -> CompanyMap<U> {
        let mut array = self.into_iter();
        CompanyMap { map: [
            f(Continental, array.next().unwrap().1),
            f(Imperial, array.next().unwrap().1),
            f(American, array.next().unwrap().1),
            f(Festival, array.next().unwrap().1),
            f(Worldwide, array.next().unwrap().1),
            f(Luxor, array.next().unwrap().1),
            f(Tower, array.next().unwrap().1),
        ]}
    }
}

impl<T: Clone> CompanyMap<T> {
    pub fn new(value: &T) -> Self {
        let initial: CompanyMap<()> = Default::default();
        initial.map(|_company, ()| value.clone())
    }
}

use std::iter::{Zip, Map};
use std::array::IntoIter;
use std::slice::{Iter, IterMut};
use std::ops::Range;

use crate::client::robust::terminal::NiceFgColor;

impl<T> IntoIterator for CompanyMap<T> {
    type Item = (Company, T);
    type IntoIter = Zip<Map<Range<u8>, fn(u8) -> Company>, IntoIter<T, 7>>;

    fn into_iter(self) -> Self::IntoIter {
        let function: fn(u8) -> Company = Company::inv_id;
        (0..7).map(function).zip(self.map.into_iter())
    }
}

impl<'a, T> IntoIterator for &'a CompanyMap<T> {
    type Item = (Company, &'a T);

    type IntoIter = Zip<Map<Range<u8>, fn(u8) -> Company>, Iter<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        let function: fn(u8) -> Company = Company::inv_id;
        (0..7).map(function).zip(self.map.iter())
    }
}

impl<'a, T> IntoIterator for &'a mut CompanyMap<T> {
    type Item = (Company, &'a mut T);

    type IntoIter = Zip<Map<Range<u8>, fn(u8) -> Company>, IterMut<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        let function: fn(u8) -> Company = Company::inv_id;
        (0..7).map(function).zip(self.map.iter_mut())
    }
}

impl<T> ops::Index<Company> for CompanyMap<T> {
    type Output = T;

    fn index(&self, index: Company) -> &Self::Output {
        &self.map[index.id()]
    }
}

impl<T> ops::IndexMut<Company> for CompanyMap<T> {
    fn index_mut(&mut self, index: Company) -> &mut Self::Output {
        &mut self.map[index.id()]
    }
}

impl<T: Serialize> Serialize for CompanyMap<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer
    {
        let mut ser = serializer.serialize_struct("CompanyMap", 7)?;
        ser.serialize_field("Continental", &self.map[0])?;
        ser.serialize_field("Imperial", &self.map[1])?;
        ser.serialize_field("American", &self.map[2])?;
        ser.serialize_field("Festival", &self.map[3])?;
        ser.serialize_field("Worldwide", &self.map[4])?;
        ser.serialize_field("Luxor", &self.map[5])?;
        ser.serialize_field("Tower", &self.map[6])?;
        ser.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CompanyMap<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        let map: CompanyMapStruct<T> = Deserialize::deserialize(deserializer)?;
        Ok(CompanyMap { map: [
            map.continental,
            map.imperial,
            map.american,
            map.worldwide,
            map.festival,
            map.luxor,
            map.tower
        ] })
    }   
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CompanyMapStruct<T> {
    continental: T,
    imperial: T,
    american: T,
    festival: T,
    worldwide: T,
    luxor: T,
    tower: T,
}
