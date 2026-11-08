//! The progress report decomp.dev reads: objdiff's `Report` message, report
//! version 2, written by hand since it is a few fields of protobuf. A report
//! is a list of units, each with its code and data bytes and the progress
//! categories it counts toward; the total and each category are the sums of
//! their units.

#[derive(Default)]
struct Wire(Vec<u8>);
impl Wire {
    fn varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.0.push(value as u8 | 0x80);
            value >>= 7;
        }
        self.0.push(value as u8);
    }
    fn uint(&mut self, field: u64, value: u64) {
        if value != 0 {
            self.varint(field << 3);
            self.varint(value);
        }
    }
    fn float(&mut self, field: u64, value: f32) {
        if value != 0.0 {
            self.varint(field << 3 | 5);
            self.0.extend(value.to_le_bytes());
        }
    }
    fn bytes(&mut self, field: u64, value: &[u8]) {
        self.varint(field << 3 | 2);
        self.varint(value.len() as u64);
        self.0.extend(value);
    }
}

#[derive(Clone, Copy, Default)]
struct Measures {
    total: u64,
    matched: u64,
    total_data: u64,
    matched_data: u64,
    units: u32,
    complete: u32,
}
impl Measures {
    fn add(&mut self, other: Measures) {
        self.total += other.total;
        self.matched += other.matched;
        self.total_data += other.total_data;
        self.matched_data += other.matched_data;
        self.units += other.units;
        self.complete += other.complete;
    }
    /// objdiff's Measures: code in total_code, matched_code and
    /// complete_code; data in total_data, matched_data and complete_data.
    fn encode(&self) -> Vec<u8> {
        let share = |matched: u64, total: u64| {
            if total == 0 {
                0.0
            } else {
                matched as f32 / total as f32 * 100.0
            }
        };
        let percent = share(self.matched, self.total);
        let data = share(self.matched_data, self.total_data);
        let mut wire = Wire::default();
        wire.float(1, percent);
        wire.uint(2, self.total);
        wire.uint(3, self.matched);
        wire.float(4, percent);
        wire.uint(5, self.total_data);
        wire.uint(6, self.matched_data);
        wire.float(7, data);
        wire.uint(11, self.matched);
        wire.float(12, percent);
        wire.uint(13, self.matched_data);
        wire.float(14, data);
        wire.uint(15, self.units.into());
        wire.uint(16, self.complete.into());
        wire.0
    }
}

/// One unit of the report.
pub(crate) struct Unit {
    pub name: String,
    /// Code bytes, and how many of them match.
    pub code: (u64, u64),
    /// Data bytes, and how many of them match.
    pub data: (u64, u64),
    pub complete: bool,
    /// The ids of the categories it counts toward.
    pub categories: Vec<String>,
}

impl Unit {
    fn measures(&self) -> Measures {
        Measures {
            total: self.code.0,
            matched: self.code.1,
            total_data: self.data.0,
            matched_data: self.data.1,
            units: 1,
            complete: self.complete.into(),
        }
    }
}

/// A progress category: its id, which units name, and the name shown.
pub(crate) struct Category {
    pub id: String,
    pub name: String,
}

/// The report's bytes: the total, each unit, then each category.
pub(crate) fn encode(units: &[Unit], categories: &[Category]) -> Vec<u8> {
    let mut total = Measures::default();
    let mut listed = Wire::default();
    for unit in units {
        total.add(unit.measures());
        let mut metadata = Wire::default();
        metadata.varint(1 << 3);
        metadata.varint(unit.complete.into());
        for category in &unit.categories {
            metadata.bytes(5, category.as_bytes());
        }
        metadata.uint(6, 1);
        let mut entry = Wire::default();
        entry.bytes(1, unit.name.as_bytes());
        entry.bytes(2, &unit.measures().encode());
        entry.bytes(5, &metadata.0);
        listed.bytes(2, &entry.0);
    }
    let mut wire = Wire::default();
    wire.bytes(1, &total.encode());
    wire.0.extend(listed.0);
    wire.uint(3, 2);
    for category in categories {
        let mut measures = Measures::default();
        for unit in units.iter().filter(|unit| unit.categories.contains(&category.id)) {
            measures.add(unit.measures());
        }
        let mut entry = Wire::default();
        entry.bytes(1, category.id.as_bytes());
        entry.bytes(2, category.name.as_bytes());
        entry.bytes(3, &measures.encode());
        wire.bytes(4, &entry.0);
    }
    wire.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_measured_beside_code() {
        let unit = Unit { name: "game/data".into(), code: (0, 0), data: (9, 7), complete: false, categories: vec!["game".into()] };
        // total_data is field 5 and matched_data field 6, as varints.
        let wire = unit.measures().encode();
        assert!(wire.windows(2).any(|pair| pair == [5 << 3, 9]));
        assert!(wire.windows(2).any(|pair| pair == [6 << 3, 7]));
        // A category sums its units: the report's total and the category's
        // measures are the unit's own.
        let report = encode(&[unit], &[Category { id: "game".into(), name: "Game".into() }]);
        let measures = Unit { name: String::new(), code: (0, 0), data: (9, 7), complete: false, categories: Vec::new() }.measures().encode();
        assert!(report.starts_with(&[1 << 3 | 2, measures.len() as u8]));
        assert!(report.ends_with(&measures));
    }
}
