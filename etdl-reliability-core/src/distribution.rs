//! Generic probability distributions (representation only; no algorithms
//! required by the ETDL compiler).

use serde::{Deserialize, Serialize};

/// A named probability distribution with parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub name: DistributionKind,
    pub parameters: Vec<f64>,
    pub domain: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistributionKind {
    Bernoulli,
    Binomial,
    Beta,
    Exponential,
    Gamma,
    Normal,
    LogNormal,
    Weibull,
    Poisson,
}

impl Distribution {
    /// Whether this distribution is supported by the reference analysis layer.
    pub fn is_supported(&self) -> bool {
        matches!(
            self.name,
            DistributionKind::Beta | DistributionKind::Exponential | DistributionKind::Normal
        )
    }

    /// Validate parameter cardinality and shape for each distribution kind.
    ///
    /// Returns `None` when valid, or a description of the first problem found.
    pub fn validate(&self) -> Option<DistributionError> {
        if self.parameters.iter().any(|p| !p.is_finite()) {
            return Some(DistributionError::NonFiniteParameter);
        }
        let expected = match self.name {
            DistributionKind::Bernoulli => Some(1),
            DistributionKind::Binomial => Some(2),
            DistributionKind::Beta => Some(2),
            DistributionKind::Exponential => Some(1),
            DistributionKind::Gamma => Some(2),
            DistributionKind::Normal => Some(2),
            DistributionKind::LogNormal => Some(2),
            DistributionKind::Weibull => Some(2),
            DistributionKind::Poisson => Some(1),
        };
        if let Some(n) = expected {
            if self.parameters.len() != n {
                return Some(DistributionError::WrongParameterCount {
                    kind: self.name,
                    expected: n,
                    got: self.parameters.len(),
                });
            }
        }
        // Shape constraints: positive scale/shape parameters.
        match self.name {
            DistributionKind::Bernoulli => {
                let p = self.parameters[0];
                if !(0.0..=1.0).contains(&p) {
                    return Some(DistributionError::ParameterOutOfRange);
                }
            }
            DistributionKind::Beta => {
                let (a, b) = (self.parameters[0], self.parameters[1]);
                if a <= 0.0 || b <= 0.0 {
                    return Some(DistributionError::ParameterOutOfRange);
                }
            }
            DistributionKind::Exponential
            | DistributionKind::Gamma
            | DistributionKind::Weibull
            | DistributionKind::Poisson => {
                if self.parameters[0] <= 0.0 {
                    return Some(DistributionError::ParameterOutOfRange);
                }
            }
            DistributionKind::Normal | DistributionKind::LogNormal | DistributionKind::Binomial => {
                if self.parameters[1] <= 0.0 {
                    return Some(DistributionError::ParameterOutOfRange);
                }
            }
        }
        if let Some((lo, hi)) = self.domain {
            if !lo.is_finite() || !hi.is_finite() || lo > hi {
                return Some(DistributionError::InvalidDomain);
            }
        }
        None
    }
}

/// Problems found while validating a distribution.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum DistributionError {
    #[error("distribution parameter is not finite (NaN or infinity)")]
    NonFiniteParameter,
    #[error("distribution {kind:?} expects {expected} parameters, got {got}")]
    WrongParameterCount {
        kind: DistributionKind,
        expected: usize,
        got: usize,
    },
    #[error("distribution parameter is outside the valid range for this distribution")]
    ParameterOutOfRange,
    #[error("distribution domain must be finite with lower <= upper")]
    InvalidDomain,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_supported() {
        let d = Distribution {
            name: DistributionKind::Beta,
            parameters: vec![3.0, 150.0],
            domain: Some((0.0, 1.0)),
        };
        assert!(d.is_supported());
        assert!(d.validate().is_none());
    }

    #[test]
    fn validates_parameter_count() {
        let d = Distribution {
            name: DistributionKind::Beta,
            parameters: vec![3.0],
            domain: None,
        };
        assert!(matches!(
            d.validate(),
            Some(DistributionError::WrongParameterCount { .. })
        ));
    }

    #[test]
    fn validates_shape_constraints() {
        // Beta requires both shape parameters > 0.
        assert!(Distribution {
            name: DistributionKind::Beta,
            parameters: vec![0.0, 5.0],
            domain: None,
        }
        .validate()
        .is_some());
        // Bernoulli probability must be in [0,1].
        assert!(Distribution {
            name: DistributionKind::Bernoulli,
            parameters: vec![1.5],
            domain: None,
        }
        .validate()
        .is_some());
        // Exponential rate must be positive.
        assert!(Distribution {
            name: DistributionKind::Exponential,
            parameters: vec![0.0],
            domain: None,
        }
        .validate()
        .is_some());
    }

    #[test]
    fn validates_domain() {
        assert!(Distribution {
            name: DistributionKind::Normal,
            parameters: vec![0.0, 1.0],
            domain: Some((1.0, 0.0)),
        }
        .validate()
        .is_some());
    }

    #[test]
    fn rejects_non_finite_parameters() {
        assert!(Distribution {
            name: DistributionKind::Normal,
            parameters: vec![0.0, f64::NAN],
            domain: None,
        }
        .validate()
        .is_some());
    }
}
