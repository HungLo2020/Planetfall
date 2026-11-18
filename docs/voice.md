# Voice

Planetfall should sound like Deep Rock Galactic: Survivor with a streak of
Borderlands: a blue-collar job in a lethal place, narrated by people who care
more about the invoice than about you. This guide is for anyone writing
player-facing text (UI, tutorial, content data, notifications).

## Who is talking

- **Mercy Salvage Ltd.** The outfit you contract for. Cheap, cheerful in
  print and ruthless in practice: every mission is a *work order*, every death
  is a *billing event*, and "Mercy" is a brand, not a policy.
- **PENNY**, the ship AI (Procurement, Expenses & Navigation, Nominally
  Yours). She runs the Mayfly, counts every credit and talks to you directly
  in the tutorial, prompts and notices. Dry, deadpan, passive-aggressive,
  quietly fond of you, mostly because replacing you is expensive.
- **The contractor** (the player). Called "Contractor", never "Commander".
  Competent, underpaid, expendable on paper.
- **The Mayfly**, your salvage cutter. Named after an insect that lives for a
  day, which PENNY considers optimistic.
- **The Kestrel Exchange** is a rival trade station: slightly shady, always
  open.

## Tone rules

1. **Instruction first, joke second.** The player must know what to do from
   the first clause. The joke is a tail, never the point.
2. **One joke per line, at most.** Deadpan beats wacky. Understatement beats
   exclamation marks.
3. **Short.** UI cards are narrow: keep descriptions within the length of the
   text they replace, and briefings to one or two sentences.
4. **Corporate deadpan for the company** (work orders, policies, fine print),
   **dry concern-trolling for PENNY**, **blunt salvage-yard names for gear**
   (stencilled-on-the-crate, not fantasy).
5. **Numbers stay exact.** Stat lines (`+{amount}% damage`) are never jokes;
   flavour lives in names and descriptions, not in the maths.
6. **Punch at the company, the bugs and the job,** never at the player's
   skill, and nothing cruel, crude or real-world.
7. Bugs are a nuisance and a commodity, not horror: "pest", "infestation",
   "billable".

## Do / don't

| Do | Don't |
|---|---|
| "Work order: mine 14 lumite. Bring it home; it doesn't count otherwise." | "Mine 14 lumite and bring it home." (flat) |
| "Signal lost. PENNY recalled your suit. The cargo stays down there." | "YOU DIED!!! LOL" |
| "Overclock: cycles {percent}% faster. Warranty void." | "Cycles {percent}% faster and also it's really cool and fast" |
| "Pest Control: kill {count} skitters." | Long paragraphs in a mission card |

## Examples

- Tutorial, welcome: "Welcome aboard the Mayfly, Contractor. It's held
  together by rust and optimism. Use {move} to walk around."
- Notification: "Not enough materials in the hold. PENNY suggests mining."
- Death: "SIGNAL LOST / PENNY recalled your suit. The cargo stays down there."
- Weapon: "Pulse Blaster: Rapid bolts at the nearest hostile. Company issue."
- Hazard levels: Routine, Billable, Hazardous, Brutal, Lethal.
