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
                "SHIP AI // WELCOME ABOARD",
                "Commander, this rust bucket is all we have left. Let's make it count. Use {move} to walk around the ship.".into(),
            ),
            TutorialStep::ToConsole => (
                "SHIP AI // NAVIGATION",
                "Walk to the Navigation Console on the bridge and press {interact} to use it.".into(),
            ),
            TutorialStep::PlotCourse => (
                "SHIP AI // PLOT A COURSE",
                "Verdant-7 is the only world our reactor can reach. Select it to engage the FTL drive.".into(),
            ),
            TutorialStep::Jumping => return None,
            TutorialStep::ToPod => (
                "SHIP AI // DROP BAY",
                if self.retry {
                    "Your suit beacon pulled you out... barely. Whatever you carried is lost. Head back to the drop pod and press {interact} when ready.".into()
                } else {
                    "We're in orbit. Head down to the drop bay and press {interact} at the pod to deploy.".into()
                },
            ),
            TutorialStep::Dropping => return None,
            TutorialStep::Survive => (
                "SHIP AI // HOSTILES",
                "Infected colonists incoming. Your weapons aim and fire automatically - just keep moving. Press {dash} to dash through danger.".into(),
            ),
            TutorialStep::Mine => (
                "SHIP AI // MINING",
                "See that glowing blue lumite in the rock? Walk into a wall to drill through it. Mine the marked lumite vein.".into(),
            ),
            TutorialStep::Collect => (
                "SHIP AI // FIELD UPGRADES",
                "Fallen hostiles drop green XP shards. Collect them to level up and pick an upgrade: rarer ones are stronger, and legendary ones change how a weapon works. Press {ability} for your class ability when they close in.".into(),
            ),
            TutorialStep::Cache => (
                "SHIP AI // SUPPLY CACHE",
                "I'm reading a supply cache nearby. Walk over it for a rare upgrade. New weapons come as you level: one at levels 5, 10 and 15. Field gear doesn't survive the trip home, so use it.".into(),
            ),
            TutorialStep::Objective => (
                "SHIP AI // OBJECTIVE",
                format!("Gather lumite to fuel our reactor: {have} / {need}. Ferrite and anything else you mine comes home too - if you extract."),
            ),
            TutorialStep::CallPod => (
                "SHIP AI // EXTRACTION",
                "That's enough lumite. Press {extract} to call the extraction pod. They'll hear it land, so be ready.".into(),
            ),
            TutorialStep::HoldZone => (
                "SHIP AI // HOLD THE ZONE",
                "The pod lands at your drop site. Get inside the ring and hold until liftoff. Die down there and the cargo is lost.".into(),
            ),
            TutorialStep::Debrief => (
                "SHIP AI // DEBRIEF",
                "Everything you carried is now in the ship's hold. Close the report when you're done.".into(),
            ),
            TutorialStep::Fabricate => (
                "SHIP AI // CONSTRUCTION",
                "Use the Fabricator in the corridor to build our first module. Upgrades are permanent and the ship grows with every one.".into(),
            ),
            TutorialStep::Done => (
                "SHIP AI // YOU'RE ON YOUR OWN",
                "That's the job: drop, mine, survive, extract, build. Visit the Locker Room to suit up as another class, and upgrade the Reactor to reach more dangerous worlds. Good hunting, Commander.".into(),
            ),
        };
        Some(text)
    }
}
