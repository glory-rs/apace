//! Opt-in, bounded engine phase measurements. Samples are inclusive; nested
//! text work and parallel worker totals must not be added to wall time.
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Phase {
    Style,
    Damage,
    Construct,
    PostConstruct,
    BoxLayout,
    Transforms,
    PaintTree,
    Cleanup,
    Hover,
    Subdocuments,
    TextShape,
    TextBreak,
    TextAlign,
}
impl Phase {
    pub const ALL: [Self; 13] = [
        Self::Style,
        Self::Damage,
        Self::Construct,
        Self::PostConstruct,
        Self::BoxLayout,
        Self::Transforms,
        Self::PaintTree,
        Self::Cleanup,
        Self::Hover,
        Self::Subdocuments,
        Self::TextShape,
        Self::TextBreak,
        Self::TextAlign,
    ];
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub calls: u64,
    /// One per stage/pass; TextShape counts prepared UTF-8 bytes, not glyphs.
    pub units: u64,
    pub elapsed: Duration,
    pub max: Duration,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot([Sample; Phase::ALL.len()]);
impl Snapshot {
    pub fn sample(&self, phase: Phase) -> Sample {
        self.0[phase as usize]
    }
    pub fn iter(&self) -> impl Iterator<Item = (Phase, Sample)> + '_ {
        Phase::ALL
            .into_iter()
            .map(|phase| (phase, self.sample(phase)))
    }
}
#[derive(Default)]
struct State {
    epoch: u64,
    samples: Snapshot,
}
/// A shared fixed-size collector, safe for parallel construction workers. No
/// node, text, frame history or application callback is retained. None on a
/// document avoids collector allocation, locks and all profiling clock reads.
#[derive(Clone, Default)]
pub struct Profile(Arc<Mutex<State>>);
impl Profile {
    pub fn snapshot(&self) -> Snapshot {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).samples
    }
    /// Retire in-flight spans atomically with clearing counters. Epochs never wrap.
    pub fn reset(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some(epoch) = state.epoch.checked_add(1) else {
            return false;
        };
        state.epoch = epoch;
        state.samples = Snapshot::default();
        true
    }
    pub fn span(&self, phase: Phase) -> Span {
        let epoch = self.0.lock().unwrap_or_else(|e| e.into_inner()).epoch;
        Span {
            profile: self.clone(),
            epoch,
            phase,
            start: Instant::now(),
            units: 1,
        }
    }
}
#[must_use]
pub struct Span {
    profile: Profile,
    epoch: u64,
    phase: Phase,
    start: Instant,
    units: u64,
}
impl Span {
    pub fn set_work(&mut self, units: u64) {
        self.units = units;
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        let mut state = self.profile.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.epoch != self.epoch {
            return;
        }
        let sample = &mut state.samples.0[self.phase as usize];
        sample.calls = sample.calls.saturating_add(1);
        sample.units = sample.units.saturating_add(self.units);
        sample.elapsed = sample.elapsed.saturating_add(elapsed);
        sample.max = sample.max.max(elapsed);
    }
}

impl crate::BaseDocument {
    /// Attaching/detaching instrumentation never damages the document or redraws.
    /// Child documents are independent; attach a shared handle explicitly to
    /// aggregate them. Subdocuments measures the parent's inclusive loop only.
    pub fn set_profile(&mut self, profile: Option<Profile>) -> Option<Profile> {
        std::mem::replace(&mut self.profile, profile)
    }
    pub fn profile(&self) -> Option<Profile> {
        self.profile.clone()
    }
    pub(crate) fn profile_span(&self, phase: Phase) -> Option<Span> {
        self.profile.as_ref().map(|profile| profile.span(phase))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_retires_old_spans_and_unwind_records_once() {
        let profile = Profile::default();
        let old = profile.span(Phase::Style);
        assert!(profile.reset());
        drop(old);
        assert_eq!(profile.snapshot(), Snapshot::default());
        let _ = std::panic::catch_unwind(|| {
            let mut span = profile.span(Phase::TextShape);
            span.set_work(7);
            panic!("profile unwind");
        });
        let sample = profile.snapshot().sample(Phase::TextShape);
        assert_eq!((sample.calls, sample.units), (1, 7));
        assert!(sample.max <= sample.elapsed);
    }
    #[test]
    fn parallel_workers_share_bounded_samples_without_losing_work() {
        let profile = Profile::default();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let profile = profile.clone();
                std::thread::spawn(move || {
                    for _ in 0..1_000 {
                        let mut span = profile.span(Phase::TextShape);
                        span.set_work(8);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let snapshot = profile.snapshot();
        let sample = snapshot.sample(Phase::TextShape);
        assert_eq!((sample.calls, sample.units), (4_000, 32_000));
        assert_eq!(snapshot.iter().count(), 13);
        assert!(sample.max <= sample.elapsed);
    }
    #[test]
    fn exhausted_epoch_is_refused_without_clearing_or_reusing_it() {
        let profile = Profile::default();
        profile.0.lock().unwrap().epoch = u64::MAX;
        drop(profile.span(Phase::BoxLayout));
        let before = profile.snapshot();
        assert!(!profile.reset());
        assert_eq!(profile.snapshot(), before);
    }
    #[test]
    fn a_document_defaults_to_no_instrumentation_and_empty_resolve_invents_no_stages() {
        let mut doc = crate::BaseDocument::new(crate::DocumentConfig::default());
        assert!(doc.profile().is_none());
        assert!(doc.profile_span(Phase::Style).is_none());
        let profile = Profile::default();
        assert!(doc.set_profile(Some(profile.clone())).is_none());
        doc.resolve(0.);
        assert_eq!(profile.snapshot(), Snapshot::default());
        assert!(doc.set_profile(None).is_some());
    }
    #[test]
    #[cfg(feature = "system-fonts")]
    fn attachment_preserves_cached_text_and_mutation_measures_actual_parley_work() {
        use crate::{Attribute, qual_name};
        let mut doc = crate::BaseDocument::new(crate::DocumentConfig {
            viewport: Some(blitz_traits::shell::Viewport::new(
                400,
                300,
                1.,
                Default::default(),
            )),
            ..Default::default()
        });
        let root = doc.root_node().id;
        let mut mutation = doc.mutate();
        let html = mutation.create_element(qual_name!("html", html), vec![]);
        let body = mutation.create_element(qual_name!("body", html), vec![]);
        let paragraph = mutation.create_element(
            qual_name!("div", html),
            vec![Attribute {
                name: qual_name!("style"),
                value: "display:block;width:160px;font-size:16px".into(),
            }],
        );
        let text = mutation.create_text_node("alpha beta gamma delta");
        mutation.append_children(root, &[html]);
        mutation.append_children(html, &[body]);
        mutation.append_children(body, &[paragraph]);
        mutation.append_children(paragraph, &[text]);
        drop(mutation);
        doc.resolve(0.);
        let size = doc.nodes[paragraph].final_layout().size;
        assert!(size.height > 0., "real system font/layout required");
        let profile = Profile::default();
        doc.set_profile(Some(profile.clone()));
        doc.resolve(0.);
        assert_eq!(doc.nodes[paragraph].final_layout().size, size);
        assert_eq!(profile.snapshot().sample(Phase::Style).calls, 1);
        assert_eq!(profile.snapshot().sample(Phase::TextShape).calls, 0);
        assert!(profile.reset());
        doc.mutate()
            .set_node_text(text, "changed text needs actual shaping");
        doc.resolve(0.);
        let snapshot = profile.snapshot();
        for phase in Phase::ALL {
            assert!(snapshot.sample(phase).calls > 0, "{phase:?}");
        }
        assert!(
            snapshot.sample(Phase::TextShape).units
                >= "changed text needs actual shaping".len() as u64
        );
        doc.set_profile(None);
        doc.mutate().set_node_text(text, "unprofiled update");
        doc.resolve(0.);
        assert_eq!(profile.snapshot(), snapshot);
    }
}
