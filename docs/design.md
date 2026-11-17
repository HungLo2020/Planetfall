# Planetfall design document (draft)

Status: draft for discussion. Describes where Planetfall is going after the
current first pass. Nothing here is implemented unless it says so; the
[gameplay doc](gameplay.md) describes the game as it is today.

## 1. Vision

A top-down horde-survival roguelite about a small salvage crew rebuilding a
ship. Each dive is a single, tense, procedurally generated cave: drop in, mine,
complete the contract, build up a weapon loadout by levelling, and get back to
the pod before the planet overwhelms you. What you bring home grows the ship
and your crew; what you find can change how every future dive plays.

**Target:** single player first, with co-op designed in from the start and
added later. No campaign: an open-ended loop of contracts, hazard levels and
progression.

### Pillars

1. **Greed against survival.** Every dive asks "one more vein, or run for the
   pod?" The planet only gets worse, and dying loses the haul.
2. **Builds you can feel.** Classes, rarities, elements and equipment combine
   into runs that play differently, not just with bigger numbers.
3. **The ship is home.** Every station is a decision (where to go, who to
   send, what to equip, what to research), not a menu in 3D.
4. **Readable chaos.** Hundreds of enemies, particles and hazards, but the
   player always knows where they are, what is dangerous and where the pod is.

## 2. The three loops

| Loop | Length | The player decides | Systems |
|---|---|---|---|
| Fight | seconds | where to move, dash, drill, use the ability | classes, weapons, enemies, hazards, elements |
| Dive | ~10–15 min | which upgrades, which weapons, when to leave | dive XP, rarity upgrades, weapon slots, objectives, escalation |
| Ship | hours | which contract, which class, what to equip, research and build | star map, locker room, equipment, class levels, modules, trade |

Each loop feeds the next: the ship sets up the dive (class, equipment,
hazard level), the dive feeds the ship (materials, class XP, equipment and
weapon finds).

## 3. The dive

### 3.1 Structure

One stage per dive, generated from a planet, a site condition and a mission.
The pod lands, the player completes the primary objective, optionally the
secondary, calls the pod and holds the zone until liftoff. The pod can be
called any time after the primary objective is complete.

### 3.2 What forces you to leave

- **Escalation.** A threat level rises for the whole dive and is shown on the
  HUD: more enemies, tougher enemies, elites mixed in, and surge waves at set
  thresholds that the HUD warns about. Extraction raises it further.
- **Diminishing growth.** The dive XP curve steepens, so upgrades arrive more
  slowly while the threat keeps accelerating. Together they form a soft cap: a
  strong build at a matching hazard level peaks around the target dive length
  and then has to leave.
- **Oxygen** is not global. It is a hazard of oxygen-poor planets and site
  conditions only (section 6).

Tuning target: measured with the autopilot soak, a well-built autopilot run
should reach its peak at roughly the target length for each hazard level.

### 3.3 Generation

Caves must feel organic and varied, not like boxes, on a single flat floor
level (no stacked levels, no pits to fall through):

- noise or cellular-automata caverns with winding tunnels and large chambers;
- set-piece rooms placed by the mission (boss arena, wreck, survivor, drill
  site, defence point);
- ore seams that follow the rock rather than scattered blocks;
- per-planet decoration and terrain features (crystal growths, lava channels,
  ice sheets, fungal patches);
- guaranteed connectivity between the pod, the objectives and extraction.

### 3.4 Death

Dying loses everything carried on that dive (materials, dive levels and
upgrades) **except finds**: equipment and weapon unlocks go to the player's
stash and unlocks immediately, even on death.

## 4. Classes

Four classes. Switching happens in the ship's **locker room**, any time, for
free. Each class keeps its own permanent record (level, unlocks, loadout and
equipped gear) per save; switching away and back loses nothing.

Every class has:

- a **signature weapon** (its default starting weapon);
- **familiar weapons** that roll more often for it, any of which can be
  chosen as the starting weapon instead (the signature then joins the pool);
- an **active ability** on a cooldown;
- a **passive**;
- a class **perk pool** on top of the shared one.

| Class | Fantasy | Signature (idea) | Active (idea) | Passive (idea) |
|---|---|---|---|---|
| Prospector | greed, mining, tunnelling | heavy drill-lance | bore a straight tunnel through rock, damaging anything in it | faster mining, richer ore drops, fragile |
| Warden | control, holding ground | arc shotgun | deploy a barrier wall | stronger turrets and barriers, extraction holds faster |
| Striker | tempo, mobility, elites | twin blades / burst rifle | blink or grapple through enemies | bonus damage to elites, faster dash recovery |
| Engineer | utility, drones, salvage | drone swarm | build a turret | repair over time, can hack beacons and salvage wrecks faster |

Names and kits are working ideas; each class should solve some planet
hazards and mission types better than the others (section 9).

### 4.1 Class progression

Two separate kinds of XP:

- **Dive XP** levels you up during a dive (upgrades and weapon unlocks) and
  resets every dive.
- **Class XP** is earned in proportion to the dive XP gained (with bonuses for
  objectives and hazard level) and is permanent.

Class levels unlock, in rough order: active and passive tiers, new perks in
the class pool, more familiar weapons, starting bonuses and cosmetics.

## 5. Weapons and upgrades

### 5.1 Weapon slots

Four slots per dive. The first holds the starting weapon. At dive levels 5,
10 and 15 the player chooses a new weapon from a random selection weighted
towards the class's familiar weapons, until all four slots are full.

### 5.2 Level-up upgrades with rarity

Other level-ups offer upgrades rolled at a rarity:

| Rarity | Typical effect |
|---|---|
| Common | small stat increase (+5 damage) |
| Uncommon | larger increase (+10) |
| Rare | larger increase (+15) or a modest unique effect |
| Legendary | +20, or a behaviour change: twin flame streams, a turret that follows you, forking chain lightning |

Rarity odds rise with the hazard level and a luck stat. Rare and legendary
upgrades are where weapons become different weapons.

### 5.3 Weapon archetypes

Today: bolt, scatter, arc, mortar, orbit. Planned: flamethrower and beam (heat
build-up), ricochet (bounces off rock), deployables (turrets, mines), melee,
homing swarm, gravity well (pull and crush). Each archetype is code; each
weapon in it is data.

### 5.4 Elements

| Element | Effect | Interactions |
|---|---|---|
| Fire | burn damage over time | ignites spore clouds; melts ice; weak on lava worlds |
| Acid | corrode: armour reduction, damage over time | dissolves certain rock and shells |
| Cold | slow, then freeze | frozen enemies shatter on heavy hits; quenches lava briefly |
| Shock | chains between nearby enemies | arcs further through water and wet enemies |
| Pierce | passes through enemies | lines up well in tunnels |

Weapons have a base element; upgrades can add more. Interactions with hazards
and with each other are what make a build feel clever.

### 5.5 Fusion

Two maxed weapons (or a maxed weapon and a specific item) can fuse into an
evolved weapon. Recipes are data and discovered by play.

## 6. Planets, sites and hazards

A planet sets the theme (palette, ore mix, enemy faction, typical hazards).
Each dive also rolls a **site condition** from that planet's pool, so the
same planet varies: a usually temperate world might roll "near the core: hot"
or "flooded caves".

Hazards (no pits):

- **Lava**: damaging channels and pools that block routes.
- **Spore clouds**: hide enemies, slow regeneration; fire ignites them.
- **Cold and heat**: stay near heat sources on cold sites; heat sites damage
  over time away from cooling vents.
- **Low oxygen**: a meter refilled at vents and caches, only on oxygen-poor
  planets and sites.
- **Quakes and collapses**: tunnels can close; the Prospector reopens them.

Planets in the long run: the current three plus new ones with their own
factions, hazards and bosses.

## 7. Enemies

Designed by role, with each planet mixing roles from its faction:

| Role | Example |
|---|---|
| Fodder | husks |
| Rushers | skitters |
| Exploders | bloaters |
| Tanks | brutes |
| Ranged | spitters that lob acid |
| Disruptors | screechers that blind or slow drilling |
| Burrowers | erupt from rock near the player |
| Flyers | ignore terrain, low health |
| Elites | any enemy with affixes (armoured, fast, splitting, regenerating) |
| Bosses | a pool per planet, with affixes at higher hazard levels |

## 8. Missions

The star map offers a mostly random board of contracts per planet.

**Primary objectives:** kill a boss; rescue a survivor and escort them to the
pod; defend a drill rig or the pod; salvage a wreck; mine an ore quota; hunt
an elite.

**Secondary objectives:** collect N of an item, kill N of a special enemy,
scan or activate points. Optional, with bonus rewards that pull the player off
the safe route.

**Mutators:** optional modifiers (rich veins, double swarm, low gravity, no
regeneration) that change rewards.

**Hazard levels:** 1 to 5, chosen per contract. Higher levels raise enemy
count, toughness and elite frequency, and give more and better loot and rarity
odds. Level N unlocks after completing a primary objective at level N-1.

## 9. How it fits together

- Planet hazards and site conditions favour certain classes, elements and
  weapons, so choosing a contract and choosing a class and loadout are one
  decision.
- Class abilities answer hazards (tunnelling around lava, barriers against
  spores, drones that hack oxygen vents).
- Elements interact with hazards, so upgrades change how a site plays.
- Equipment shapes builds before the dive; rarity upgrades shape them during.
- Escalation and steepening XP make every dive end in a greed decision.

## 10. Equipment and the stash

- Rare finds in a dive: equipment (randomised like loot games) and weapon
  unlocks. Both survive death.
- **Four slots per class:** Helmet, Suit, Boots and Gadget (the gadget holds
  the most unusual effects).
- Items have a rarity, a base type and one to four random affixes; higher
  hazard levels roll better items.
- The **stash** in the locker room holds everything found. Extras can be sold
  or traded at a space station, or fused into stronger items.
- **Weapon caches** in dives become the main source of equipment and weapon
  unlocks, plus a guaranteed rare-or-better upgrade.

## 11. The ship

| Station | Purpose |
|---|---|
| Star map | contracts, hazard levels, travel to planets and the station |
| Locker room | class switching, loadout and starting weapon, equipment, stash |
| Fabricator | ship modules (today's upgrades) |
| Research lab | a tech tree fed by rare materials |
| Hangar | drop pod upgrades (supply call-ins, faster pod) |
| Trade station | a destination on the star map: sell and buy items, trade materials, credits |

Free-form building stays. Ship modules and research are shared across
classes; class progress is per class.

### Economy

Ferrite, lumite and voidstone still pay for the ship. Selling equipment earns
**credits**, spent at the trade station (items, rerolls, material trades).

## 12. Presentation

- **Art direction:** consistent stylised low-poly, readability first.
- **Models:** move from the Python shape generator towards hand-authored
  models (Blender), keeping the generator for prototypes. Characters need
  class silhouettes, enemies need role silhouettes.
- **Effects:** Quasar 0.2.25's particles, decals, outlines, fog and clustered
  lights carry elements and hazards (burning, frozen, shocked states).
- **UI:** a threat meter, surge warnings, rarity colours, equipment
  comparison and a mission board.

## 13. Technical direction

- **Data first:** classes, weapons, upgrades, rarities, elements, equipment
  affixes, missions, site conditions, mutators and hazards as validated
  content tables; behaviours stay small archetype enums in code.
- **Co-op ready:** the simulation already runs deterministically from inputs,
  which lockstep networking needs (`quasar-net`). Model players as a list
  (per-player position, class, loadout, upgrades and inputs) from the next
  milestone on, even with one player.
- **Saves:** class records, stash and unlocks per save slot, with versioned
  migration as today.
- **Testing:** the autopilot learns each new mechanic so soak runs keep
  measuring balance and dive length.

## 14. Roadmap

Each milestone is playable and releasable. Status: stages 1 to 8 are done.

1. **Classes and the locker room.** Four classes (ability, passive,
   signature and familiar weapons), per-class records, class XP, players
   modelled as a list.
2. **Rarity upgrades and weapon slots.** Rarity tiers, legendary behaviour
   changes, weapon picks at levels 5, 10 and 15, elements without
   interactions.
3. **Escalation and dive length.** Threat level, surges, the steeper XP
   curve, tuning with the autopilot.
4. **Mission board and hazard levels.** Primary and secondary objectives,
   site conditions, mutators, hazard levels 1 to 5.
5. **Organic generation.** New cave generator and set-piece rooms.
6. **Equipment and the stash.** Randomised gear, four slots, caches
   rewritten, the trade station and credits.
7. **Content expansion.** New weapons, archetypes, element interactions,
   enemies by role, elites and bosses, new planets and hazards.
8. **Art pass.** Authored models, class and enemy silhouettes, effects for
   elements and hazards.
9. **Co-op.** Lockstep sessions and shared extraction, once Basalt allows
   network use.

## 15. Open questions

- Final class names, kits and signature weapons.
- What exactly an escalation "surge" is and how often it comes.
- Boss design per planet.
- Setting and tone beyond the dry ship AI.
- Research tree contents and how it differs from ship modules.
- Co-op specifics: shared or separate XP, revives, scaling.
