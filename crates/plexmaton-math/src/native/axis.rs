//! Monotone cell coordinates with explicit minimum separations; no TeX layout rules live here.
use crate::{Limit, MAX_DIMENSION, MathError};

const PRECISION: f64 = 1_000_000.0;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Coordinate(i64);

impl Coordinate {
    fn new(value: f64) -> Result<Self, MathError> {
        if !value.is_finite() || value.abs() > 1024.0 {
            return Err(MathError::Limited(Limit::Geometry));
        }
        Ok(Self((value * PRECISION).round() as i64))
    }
}

pub(super) struct Axis {
    coordinates: Vec<Coordinate>,
    incoming: Vec<Vec<(usize, usize)>>,
    values: Vec<usize>,
    density: f64,
}

impl Axis {
    pub(super) fn new(points: &[f64], density: f64) -> Result<Self, MathError> {
        let mut coordinates = points
            .iter()
            .copied()
            .map(Coordinate::new)
            .collect::<Result<Vec<_>, _>>()?;
        coordinates.sort_unstable();
        coordinates.dedup();
        Ok(Self {
            incoming: vec![Vec::new(); coordinates.len()],
            values: vec![0; coordinates.len()],
            coordinates,
            density,
        })
    }

    fn index(&self, value: f64) -> Result<usize, MathError> {
        self.coordinates
            .binary_search(&Coordinate::new(value)?)
            .map_err(|_| MathError::Limited(Limit::Geometry))
    }

    pub(super) fn require(&mut self, from: f64, to: f64, gap: usize) -> Result<(), MathError> {
        let from = self.index(from)?;
        let to = self.index(to)?;
        if from > to || (from == to && gap > 0) {
            return Err(MathError::Overlap);
        }
        if from != to {
            self.incoming[to].push((from, gap));
        }
        Ok(())
    }

    pub(super) fn solve(&mut self) -> Result<(), MathError> {
        for index in 0..self.coordinates.len() {
            self.advance(index)?;
        }
        Ok(())
    }

    pub(super) fn len(&self) -> usize {
        self.coordinates.len()
    }

    pub(super) fn advance(&mut self, index: usize) -> Result<(), MathError> {
        let offset = self.coordinates[index].0 - self.coordinates[0].0;
        let preferred = ((offset as f64 / PRECISION) * self.density).round() as usize;
        let previous = index.checked_sub(1).map_or(0, |p| self.values[p]);
        let mut value = preferred.max(previous);
        for (from, gap) in &self.incoming[index] {
            value = value.max(self.values[*from] + gap);
        }
        if value > MAX_DIMENSION {
            return Err(MathError::Limited(Limit::Geometry));
        }
        self.values[index] = value;
        Ok(())
    }

    pub(super) fn at(&self, point: f64) -> Result<usize, MathError> {
        Ok(self.values[self.index(point)?])
    }
    pub(super) fn position(&self, point: f64) -> Result<usize, MathError> {
        self.index(point)
    }

    pub(super) fn raise(&mut self, index: usize, minimum: usize) -> Result<(), MathError> {
        if minimum > MAX_DIMENSION {
            return Err(MathError::Limited(Limit::Geometry));
        }
        self.values[index] = self.values[index].max(minimum);
        Ok(())
    }
}
