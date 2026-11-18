//! First-launch tutorial: walks the player from the barebones ship through
//! their first FTL jump, drop, extraction and ship upgrade.
//!
//! Prompt text uses `{move}`, `{interact}`, `{dash}`, `{extract}` and `{ability}`
//! placeholders that the presentation layer replaces with device labels.

use crate::math::V2;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum TutorialStep {
    Welcome,
    ToConsole,
    PlotCourse,
    Jumping,
    ToPod,
    Dropping,
    Survive,
    Mine,
    Collect,
    Cache,
    Objective,
    CallPod,
    HoldZone,
    Debrief,
    Fabricate,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub title: &'static str,
    pub body: String,
    /// World point to highlight with a marker, in the current phase's space.
    pub target: Option<V2>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tutorial {
    pub step: TutorialStep,
    /// Ticks spent in the current step.
    pub ticks: u32,
    /// Set after a failed tutorial drop to change the pod prompt.
    pub retry: bool,
}

impl Tutorial {
    pub fn new(step: TutorialStep) -> Self {
        Self {
            step,
            ticks: 0,
            retry: false,
        }
    }

    pub fn advance(&mut self, step: TutorialStep) -> bool {
        if step != self.step {
            self.step = step;
            self.ticks = 0;
            true
        } else {
            false
        }
    }

    /// Static wording for a step; `Game` fills in targets and live numbers.
    pub fn text(&self, have: u32, need: u32) -> Option<(&'static str, String)> {
        let text = match self.step {
            TutorialStep::Welcome => (
                "PENNY // WELCOME ABOARD",
                "Welcome aboard the Mayfly, Contractor. She's held together by rust and optimism. Use {move} to walk around.".into(),
            ),
            TutorialStep::ToConsole => (
                "PENNY // NAVIGATION",
                "Walk to the Navigation Console on the bridge and press {interact}. Mind the loose panel.".into(),
            ),
            TutorialStep::PlotCourse => (
                "PENNY // PLOT A COURSE",
                "Verdant-7 is the only world our reactor can afford. Select it to engage the FTL drive.".into(),
            ),
            TutorialStep::Jumping => return None,
            TutorialStep::ToPod => (
                "PENNY // DROP BAY",
                if self.retry {
                    "I recalled your suit. The cargo stayed down there, and so did your dignity. Back to the pod; press {interact} when ready.".into()
                } else {
                    "We're in orbit. Head to the drop bay and press {interact} at the pod. The pod is rated for one landing. Probably.".into()
                },
            ),
            TutorialStep::Dropping => return None,
            TutorialStep::Survive => (
                "PENNY // PESTS",
                "Infestation incoming. Your weapons aim and fire on their own, so just keep moving. Press {dash} to dash through trouble.".into(),
            ),
            TutorialStep::Mine => (
                "PENNY // MINING",
                "See the glowing blue lumite? That's payroll. Walk into a wall to drill it. Mine the marked lumite vein.".into(),
            ),
            TutorialStep::Collect => (
                "PENNY // FIELD UPGRADES",
                "Dead bugs drop green XP shards. Collect them to level up and pick an upgrade: rarer is stronger, and legendaries change how a weapon works. Press {ability} for your class ability when they crowd you.".into(),
            ),
            TutorialStep::Cache => (
                "PENNY // SUPPLY CACHE",
                "Supply cache nearby. Walk over it for a rare upgrade. New weapons arrive at levels 5, 10 and 15. Field gear doesn't survive the trip home, so use it up.".into(),
            ),
            TutorialStep::Objective => (
                "PENNY // WORK ORDER",
                format!("Gather lumite for the reactor: {have} / {need}. Everything else you mine comes home too, if you do."),
            ),
            TutorialStep::CallPod => (
                "PENNY // EXTRACTION",
                "That's quota. Press {extract} to call the extraction pod. It is not quiet, and they will notice.".into(),
            ),
            TutorialStep::HoldZone => (
                "PENNY // HOLD THE ZONE",
                "The pod lands at your drop site. Stand in the ring until liftoff. Die down there and the cargo is lost. And docked.".into(),
            ),
            TutorialStep::Debrief => (
                "PENNY // DEBRIEF",
                "Your haul is in the hold. I've started the paperwork. Close the report when you're done.".into(),
            ),
            TutorialStep::Fabricate => (
                "PENNY // CONSTRUCTION",
                "Use the Fabricator in the corridor to build our first module. Upgrades are permanent; the ship grows with each one.".into(),
            ),
            TutorialStep::Done => (
                "PENNY // YOU'RE ON YOUR OWN",
                "That's the job: drop, mine, survive, extract, build. Try another class in the Locker Room, and upgrade the Reactor for richer, nastier worlds. Mercy Salvage thanks you for your service.".into(),
            ),
        };
        Some(text)
    }
}
