//! A shuffle that still flows.
//!
//! A plain shuffle can follow a 70 BPM ballad with a 170 BPM stomper. This
//! one is random too, but each next song is picked from the few whose tempo
//! is closest to the last one, so a list drifts up and down instead of
//! jumping. Songs with no known tempo are dropped in at random places.

/// A tiny generator, so a shuffle needs no extra crate and tests can use a
/// fixed seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// How far apart two tempos feel. Half and double time sound alike, so 70
/// and 140 are close.
pub fn tempo_gap(a: f32, b: f32) -> f32 {
    let direct = (a - b).abs();
    let half = (a * 2.0 - b).abs();
    let double = (a - b * 2.0).abs();
    direct.min(half * 1.5).min(double * 1.5)
}

/// The order to play `items` in: `(id, tempo)` pairs in, ids out.
pub fn smart_order(items: &[(String, Option<f32>)], rng: &mut Rng) -> Vec<String> {
    let mut with: Vec<(String, f32)> = items
        .iter()
        .filter_map(|(id, tempo)| tempo.map(|t| (id.clone(), t)))
        .collect();
    let without: Vec<String> = items
        .iter()
        .filter(|(_, tempo)| tempo.is_none())
        .map(|(id, _)| id.clone())
        .collect();
    let mut ordered: Vec<String> = Vec::with_capacity(items.len());
    if !with.is_empty() {
        let first = rng.below(with.len());
        let (id, mut last) = with.swap_remove(first);
        ordered.push(id);
        while !with.is_empty() {
            // The five closest in tempo; one of them at random, the closer
            // ones more likely.
            let mut ranked: Vec<(usize, f32)> = with
                .iter()
                .enumerate()
                .map(|(i, (_, t))| (i, tempo_gap(last, *t)))
                .collect();
            ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
            ranked.truncate(5);
            let weights: Vec<usize> = (0..ranked.len()).map(|i| ranked.len() - i).collect();
            let total: usize = weights.iter().sum();
            let mut roll = rng.below(total);
            let mut pick = 0;
            for (i, w) in weights.iter().enumerate() {
                if roll < *w {
                    pick = i;
                    break;
                }
                roll -= *w;
            }
            let (id, tempo) = with.swap_remove(ranked[pick].0);
            last = tempo;
            ordered.push(id);
        }
    }
    for id in without {
        let at = rng.below(ordered.len() + 1);
        ordered.insert(at, id);
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(tempos: &[Option<f32>]) -> Vec<(String, Option<f32>)> {
        tempos
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("s{i}"), *t))
            .collect()
    }

    #[test]
    fn every_song_appears_once() {
        let items = songs(&[Some(80.0), None, Some(128.0), Some(90.0), None, Some(170.0)]);
        let mut order = smart_order(&items, &mut Rng::new(7));
        assert_eq!(order.len(), items.len());
        order.sort();
        order.dedup();
        assert_eq!(order.len(), items.len());
    }

    #[test]
    fn neighbours_are_closer_than_a_plain_shuffle() {
        let tempos: Vec<Option<f32>> = (0..60).map(|i| Some(70.0 + (i * 37 % 100) as f32)).collect();
        let items = songs(&tempos);
        let tempo_of = |id: &String| tempos[id[1..].parse::<usize>().unwrap()].unwrap();
        let jump = |order: &[String]| -> f32 {
            order.windows(2).map(|w| (tempo_of(&w[0]) - tempo_of(&w[1])).abs()).sum::<f32>() / (order.len() - 1) as f32
        };
        let smart = jump(&smart_order(&items, &mut Rng::new(3)));
        let mut rng = Rng::new(3);
        let mut plain: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
        for i in (1..plain.len()).rev() {
            plain.swap(i, rng.below(i + 1));
        }
        assert!(smart < jump(&plain) * 0.6, "{smart} vs {}", jump(&plain));
    }

    #[test]
    fn half_time_counts_as_close() {
        assert!(tempo_gap(70.0, 140.0) < tempo_gap(70.0, 110.0));
    }

    #[test]
    fn empty_and_single_lists_work() {
        assert!(smart_order(&[], &mut Rng::new(1)).is_empty());
        assert_eq!(smart_order(&songs(&[None]), &mut Rng::new(1)).len(), 1);
    }
}
