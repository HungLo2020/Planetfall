//! Seeded choices on top of `quasar_core::Rng`.

use quasar_core::Rng;

/// One of `choices` with probability proportional to its integer weight
/// (one `range_u32` draw), or `None` when every weight is zero.
/// `Rng::weighted_index` takes `f64` weights and draws differently, so
/// switching would change every seeded run.
pub fn weighted<T: Copy>(rng: &mut Rng, choices: &[(T, u32)]) -> Option<T> {
    let total: u32 = choices.iter().map(|(_, w)| w).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.range_u32(0..total);
    for &(choice, weight) in choices {
        if roll < weight {
            return Some(choice);
        }
        roll -= weight;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_bias_choices_and_zero_is_never_picked() {
        let mut rng = Rng::seed_from_u64(3);
        let choices = [('a', 1), ('b', 0), ('c', 3)];
        let picks: Vec<char> = (0..400)
            .filter_map(|_| weighted(&mut rng, &choices))
            .collect();
        let count = |c| picks.iter().filter(|&&p| p == c).count();
        assert_eq!(picks.len(), 400);
        assert_eq!(count('b'), 0);
        assert!(count('c') > count('a') * 2);
        assert_eq!(weighted(&mut rng, &[('z', 0)]), None);
    }
}
