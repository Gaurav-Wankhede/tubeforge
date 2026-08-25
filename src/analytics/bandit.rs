//! Contextual Multi-Armed Bandit (LinUCB) for dynamic packaging optimization.
//!
//! Replaces static A/B testing with a contextual reinforcement learning engine
//! (LinUCB) that balances exploration and exploitation in real time, minimizing
//! cumulative sample regret when serving thumbnail variants and title candidates.
//!
//! Mathematical model:
//!   Expected Reward: E[R_t | X_t, a] = X_t^T * theta_a
//!   Upper Confidence Bound: UCB_a = X_t^T * \hat{theta}_a + alpha * sqrt(X_t^T * A_a^{-1} * X_t)
//! where A_a = D_a^T * D_a + I_d, and b_a = D_a^T * c_a.

use serde::{Deserialize, Serialize};

/// Context features for a viewer/impression cohort.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerContext {
    /// Normalized feature vector (e.g. [is_mobile, is_subscriber, affinity_systems, affinity_ai, is_peak_hour]).
    pub features: Vec<f64>,
}

impl ViewerContext {
    pub fn new(features: Vec<f64>) -> Self {
        Self { features }
    }

    /// Standard 4-dimensional technical viewer context:
    /// [is_mobile (0/1), is_subscriber (0/1), high_intent_search (0/1), evening_browse (0/1)]
    pub fn standard(is_mobile: bool, is_sub: bool, search_intent: bool, evening: bool) -> Self {
        Self {
            features: vec![
                if is_mobile { 1.0 } else { 0.0 },
                if is_sub { 1.0 } else { 0.0 },
                if search_intent { 1.0 } else { 0.0 },
                if evening { 1.0 } else { 0.0 },
            ],
        }
    }
}

/// A candidate visual asset or title variation (Arm in MAB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmCandidate {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// LinUCB arm state tracking ridge regression matrices: A_a (d x d) and b_a (d x 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinUcbArmState {
    pub arm_id: String,
    pub dimension: usize,
    /// Flattened (d x d) matrix initialized to Identity.
    pub matrix_a: Vec<f64>,
    /// (d x 1) vector initialized to zeros.
    pub vector_b: Vec<f64>,
    pub pull_count: u64,
    pub total_reward: f64,
}

impl LinUcbArmState {
    pub fn new(arm_id: &str, d: usize) -> Self {
        let mut matrix_a = vec![0.0; d * d];
        for i in 0..d {
            matrix_a[i * d + i] = 1.0; // Identity matrix
        }
        Self {
            arm_id: arm_id.to_string(),
            dimension: d,
            matrix_a,
            vector_b: vec![0.0; d],
            pull_count: 0,
            total_reward: 0.0,
        }
    }

    /// Invert the d x d matrix A using Gauss-Jordan elimination.
    pub fn invert_a(&self) -> Option<Vec<f64>> {
        let d = self.dimension;
        let mut aug = vec![0.0; d * 2 * d];

        for i in 0..d {
            for j in 0..d {
                aug[i * 2 * d + j] = self.matrix_a[i * d + j];
            }
            aug[i * 2 * d + (d + i)] = 1.0;
        }

        for i in 0..d {
            let mut pivot = aug[i * 2 * d + i];
            if pivot.abs() < 1e-9 {
                let mut swap_row = None;
                for k in (i + 1)..d {
                    if aug[k * 2 * d + i].abs() > 1e-9 {
                        swap_row = Some(k);
                        break;
                    }
                }
                if let Some(r) = swap_row {
                    for col in 0..(2 * d) {
                        let tmp = aug[i * 2 * d + col];
                        aug[i * 2 * d + col] = aug[r * 2 * d + col];
                        aug[r * 2 * d + col] = tmp;
                    }
                    pivot = aug[i * 2 * d + i];
                } else {
                    return None; // Singular matrix
                }
            }

            let inv_pivot = 1.0 / pivot;
            for col in 0..(2 * d) {
                aug[i * 2 * d + col] *= inv_pivot;
            }

            for row in 0..d {
                if row != i {
                    let factor = aug[row * 2 * d + i];
                    for col in 0..(2 * d) {
                        aug[row * 2 * d + col] -= factor * aug[i * 2 * d + col];
                    }
                }
            }
        }

        let mut inv = vec![0.0; d * d];
        for i in 0..d {
            for j in 0..d {
                inv[i * d + j] = aug[i * 2 * d + (d + j)];
            }
        }
        Some(inv)
    }

    /// Compute expected reward + UCB exploration bonus:
    /// score = x^T * theta + alpha * sqrt(x^T * A^{-1} * x)
    pub fn compute_score(&self, context: &ViewerContext, alpha: f64) -> f64 {
        let d = self.dimension;
        if context.features.len() != d {
            return 0.0;
        }

        let inv_a = match self.invert_a() {
            Some(inv) => inv,
            None => return 0.0,
        };

        // theta = A^{-1} * b
        let mut theta = vec![0.0; d];
        for i in 0..d {
            let mut sum = 0.0;
            for j in 0..d {
                sum += inv_a[i * d + j] * self.vector_b[j];
            }
            theta[i] = sum;
        }

        // expected_payoff = x^T * theta
        let mut expected_payoff = 0.0;
        for i in 0..d {
            expected_payoff += context.features[i] * theta[i];
        }

        // var = x^T * A^{-1} * x
        let mut inv_a_x = vec![0.0; d];
        for i in 0..d {
            let mut sum = 0.0;
            for j in 0..d {
                sum += inv_a[i * d + j] * context.features[j];
            }
            inv_a_x[i] = sum;
        }

        let mut variance = 0.0;
        for i in 0..d {
            variance += context.features[i] * inv_a_x[i];
        }

        let ucb_bonus = alpha * variance.max(0.0).sqrt();
        expected_payoff + ucb_bonus
    }

    /// Update A_a += x * x^T and b_a += reward * x
    pub fn update(&mut self, context: &ViewerContext, reward: f64) {
        let d = self.dimension;
        if context.features.len() != d {
            return;
        }

        for i in 0..d {
            for j in 0..d {
                self.matrix_a[i * d + j] += context.features[i] * context.features[j];
            }
            self.vector_b[i] += reward * context.features[i];
        }
        self.pull_count += 1;
        self.total_reward += reward;
    }
}

/// Contextual Multi-Armed Bandit Selector across variant candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinUcbEngine {
    pub arms: Vec<LinUcbArmState>,
    pub alpha: f64,
    pub dimension: usize,
}

impl LinUcbEngine {
    pub fn new(arm_ids: &[&str], dimension: usize, alpha: f64) -> Self {
        let arms = arm_ids
            .iter()
            .map(|id| LinUcbArmState::new(id, dimension))
            .collect();
        Self {
            arms,
            alpha,
            dimension,
        }
    }

    /// Select the best arm for the given viewer context using LinUCB.
    pub fn select_arm(&self, context: &ViewerContext) -> Option<(String, f64)> {
        let mut best_arm: Option<String> = None;
        let mut max_score = f64::NEG_INFINITY;

        for arm in &self.arms {
            let score = arm.compute_score(context, self.alpha);
            if score > max_score {
                max_score = score;
                best_arm = Some(arm.arm_id.clone());
            }
        }

        best_arm.map(|id| (id, max_score))
    }

    /// Record impression click / retention feedback (Reward in [0.0, 1.0]).
    pub fn record_feedback(&mut self, arm_id: &str, context: &ViewerContext, reward: f64) -> bool {
        if let Some(arm) = self.arms.iter_mut().find(|a| a.arm_id == arm_id) {
            arm.update(context, reward.clamp(0.0, 1.0));
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linucb_arm_selection_and_learning() {
        let mut engine = LinUcbEngine::new(&["thumb_a", "thumb_b", "thumb_c"], 4, 0.5);
        let mobile_sub = ViewerContext::standard(true, true, false, true);

        let (initial_arm, score) = engine.select_arm(&mobile_sub).expect("arm selected");
        assert!(!initial_arm.is_empty());
        assert!(score >= 0.0);

        for _ in 0..10 {
            engine.record_feedback("thumb_b", &mobile_sub, 1.0);
            engine.record_feedback("thumb_a", &mobile_sub, 0.1);
        }

        let (selected, _) = engine.select_arm(&mobile_sub).expect("selected");
        assert_eq!(selected, "thumb_b");
    }
}
