use super::{GitHubSponsor, Ledger, SponsorsActivityAction};
use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SponsorKey {
    Id(i64),
    Login(String),
}

impl SponsorKey {
    fn of(sponsor: &GitHubSponsor) -> Self {
        match sponsor.database_id {
            Some(id) => Self::Id(id),
            None => Self::Login(sponsor.login.clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct Spell {
    activity_id: String,
    start: chrono::DateTime<chrono::Utc>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    segments: Vec<(chrono::DateTime<chrono::Utc>, i64)>,
}

impl Spell {
    #[inline]
    fn monthly_in_cents(&self) -> i64 {
        self.segments.last().map(|(_, cents)| *cents).unwrap_or(0)
    }

    fn evaluate(&self, now: chrono::DateTime<chrono::Utc>) -> (i64, u32) {
        let end = self.end.unwrap_or(now);

        let mut total = 0;
        let mut months = 0;

        for elapsed in 0.. {
            let Some(charged) = self.start.checked_add_months(chrono::Months::new(elapsed)) else {
                break;
            };

            if charged > end {
                break;
            }

            total += self
                .segments
                .iter()
                .rev()
                .find(|(from, _)| *from <= charged)
                .map(|(_, cents)| *cents)
                .unwrap_or(0);
            months += 1;
        }

        (total, months)
    }
}

#[derive(Debug, Clone)]
pub struct EvaluatedSpell {
    pub activity_id: String,
    pub sponsor: Option<GitHubSponsor>,
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: Option<chrono::DateTime<chrono::Utc>>,
    pub monthly_in_cents: i64,
    pub paid_in_cents: i64,
    pub months_paid: u32,
}

impl EvaluatedSpell {
    #[inline]
    pub fn active(&self) -> bool {
        self.end.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct EvaluatedSponsor {
    pub sponsor: Option<GitHubSponsor>,

    pub active: bool,
    pub recurring_spells: usize,
    pub monthly_in_cents: i64,

    pub one_time_in_cents: i64,
    pub recurring_in_cents: i64,
    pub lifetime_in_cents: i64,
    pub estimated_months_paid: u32,

    pub first_sponsored_at: chrono::DateTime<chrono::Utc>,
    pub last_activity_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct Evaluation {
    pub monthly_recurring_in_cents: i64,

    pub one_time_in_cents: i64,
    pub recurring_in_cents: i64,
    pub lifetime_in_cents: i64,

    pub active_sponsor_count: usize,
    pub former_sponsor_count: usize,
    pub one_time_sponsor_count: usize,

    pub sponsors: Vec<EvaluatedSponsor>,
    pub spells: Vec<EvaluatedSpell>,
}

#[derive(Default)]
struct SponsorState {
    sponsor: Option<GitHubSponsor>,
    public: bool,
    one_time_in_cents: i64,
    one_time_refunded_in_cents: i64,
    recurring_refunded_in_cents: i64,
    spells: Vec<Spell>,
    first_sponsored_at: Option<chrono::DateTime<chrono::Utc>>,
    last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn evaluate(ledger: &Ledger, now: chrono::DateTime<chrono::Utc>) -> Evaluation {
    let mut states: IndexMap<SponsorKey, SponsorState> = IndexMap::new();

    for activity in &ledger.activities {
        let (Some(action), Some(sponsor), Some(timestamp)) = (
            activity.action,
            activity.sponsor.as_ref(),
            activity.timestamp,
        ) else {
            continue;
        };

        let state = states.entry(SponsorKey::of(sponsor)).or_default();
        state.sponsor = Some(sponsor.clone());
        state.public = activity.is_public();
        state.first_sponsored_at.get_or_insert(timestamp);
        state.last_activity_at = Some(timestamp);

        match action {
            SponsorsActivityAction::NewSponsorship => {
                let Some(tier) = activity.sponsors_tier.as_ref() else {
                    continue;
                };

                if tier.is_one_time {
                    state.one_time_in_cents += tier.monthly_price_in_cents;
                } else {
                    if let Some(open) = state.spells.iter_mut().find(|s| s.end.is_none()) {
                        open.end = Some(timestamp);
                    }

                    state.spells.push(Spell {
                        activity_id: activity.id.clone(),
                        start: timestamp,
                        end: None,
                        segments: vec![(timestamp, tier.monthly_price_in_cents)],
                    });
                }
            }
            SponsorsActivityAction::TierChange => {
                let Some(tier) = activity.sponsors_tier.as_ref() else {
                    continue;
                };

                if let Some(open) = state.spells.iter_mut().find(|s| s.end.is_none()) {
                    open.segments.push((timestamp, tier.monthly_price_in_cents));
                }
            }
            SponsorsActivityAction::CancelledSponsorship => {
                if let Some(open) = state.spells.iter_mut().find(|s| s.end.is_none()) {
                    open.end = Some(timestamp);
                }
            }
            SponsorsActivityAction::Refund => {
                if let Some(tier) = activity.sponsors_tier.as_ref() {
                    if tier.is_one_time {
                        state.one_time_refunded_in_cents += tier.monthly_price_in_cents;
                    } else {
                        state.recurring_refunded_in_cents += tier.monthly_price_in_cents;
                    }
                }
            }
            SponsorsActivityAction::PendingChange
            | SponsorsActivityAction::SponsorMatchDisabled => {}
        }
    }

    let mut evaluation = Evaluation {
        monthly_recurring_in_cents: 0,
        one_time_in_cents: 0,
        recurring_in_cents: 0,
        lifetime_in_cents: 0,
        active_sponsor_count: 0,
        former_sponsor_count: 0,
        one_time_sponsor_count: 0,
        sponsors: Vec::new(),
        spells: Vec::new(),
    };

    for state in states.into_values() {
        let (Some(sponsor), Some(first_sponsored_at), Some(last_activity_at)) = (
            state.sponsor,
            state.first_sponsored_at,
            state.last_activity_at,
        ) else {
            continue;
        };

        let mut recurring_in_cents = 0;
        let mut estimated_months_paid = 0;
        let mut monthly_in_cents = 0;
        let mut recurring = false;

        for spell in &state.spells {
            let (paid_in_cents, months_paid) = spell.evaluate(now);

            recurring_in_cents += paid_in_cents;
            estimated_months_paid += months_paid;

            if spell.end.is_none() {
                recurring = true;
                monthly_in_cents += spell.monthly_in_cents();
            }

            evaluation.spells.push(EvaluatedSpell {
                activity_id: spell.activity_id.clone(),
                sponsor: state.public.then(|| sponsor.clone()),
                start: spell.start,
                end: spell.end,
                monthly_in_cents: spell.monthly_in_cents(),
                paid_in_cents,
                months_paid,
            });
        }

        let one_time_in_cents = state.one_time_in_cents - state.one_time_refunded_in_cents;
        let recurring_in_cents = recurring_in_cents - state.recurring_refunded_in_cents;
        let lifetime_in_cents = one_time_in_cents + recurring_in_cents;

        evaluation.monthly_recurring_in_cents += monthly_in_cents;
        evaluation.one_time_in_cents += one_time_in_cents;
        evaluation.recurring_in_cents += recurring_in_cents;
        evaluation.lifetime_in_cents += lifetime_in_cents;

        if recurring {
            evaluation.active_sponsor_count += 1;
        } else if !state.spells.is_empty() {
            evaluation.former_sponsor_count += 1;
        } else {
            evaluation.one_time_sponsor_count += 1;
        }

        evaluation.sponsors.push(EvaluatedSponsor {
            sponsor: state.public.then_some(sponsor),

            active: recurring,
            recurring_spells: state.spells.len(),
            monthly_in_cents,

            one_time_in_cents,
            recurring_in_cents,
            lifetime_in_cents,
            estimated_months_paid,

            first_sponsored_at,
            last_activity_at,
        });
    }

    if let Some(reported) = ledger.monthly_estimated_income_in_cents
        && reported != evaluation.monthly_recurring_in_cents
    {
        tracing::warn!(
            "reconstructed monthly sponsorship income ({} cents) does not match github's own figure ({} cents)",
            evaluation.monthly_recurring_in_cents,
            reported
        );
    }

    evaluation
        .sponsors
        .sort_by_key(|sponsor| std::cmp::Reverse(sponsor.lifetime_in_cents));
    evaluation.spells.sort_by_key(|spell| spell.start);

    evaluation
}
