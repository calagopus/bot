use super::{Contribution, Ledger, Provider, SponsorAccount};
use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct EvaluatedSponsor {
    pub provider: Provider,
    /// Matches `linked_accounts.provider_id` whenever the contributor has a real account.
    pub key: String,
    /// `None` when every one of their contributions is private or incognito.
    pub account: Option<SponsorAccount>,

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
    pub currency: String,

    pub monthly_recurring_in_cents: i64,

    pub one_time_in_cents: i64,
    pub recurring_in_cents: i64,
    pub lifetime_in_cents: i64,

    pub active_sponsor_count: usize,
    pub former_sponsor_count: usize,
    pub one_time_sponsor_count: usize,

    pub sponsors: Vec<EvaluatedSponsor>,
    pub contributions: Vec<Contribution>,
}

impl Evaluation {
    #[inline]
    pub fn sponsor(&self, provider: Provider, key: &str) -> Option<&EvaluatedSponsor> {
        self.sponsors
            .iter()
            .find(|sponsor| sponsor.provider == provider && sponsor.key == key)
    }

    pub fn totals(&self, accounts: &[(Provider, String)]) -> (i64, i64) {
        accounts
            .iter()
            .filter_map(|(provider, key)| self.sponsor(*provider, key))
            .fold((0, 0), |(lifetime, monthly), sponsor| {
                (
                    lifetime + sponsor.lifetime_in_cents,
                    monthly + sponsor.monthly_in_cents,
                )
            })
    }
}

#[derive(Default)]
struct SponsorState {
    account: Option<SponsorAccount>,
    contributions: Vec<Contribution>,
}

pub fn evaluate(ledger: &Ledger) -> Evaluation {
    let mut states: IndexMap<(Provider, String), SponsorState> = IndexMap::new();

    let mut evaluation = Evaluation {
        currency: ledger.currency.clone(),

        monthly_recurring_in_cents: 0,
        one_time_in_cents: 0,
        recurring_in_cents: 0,
        lifetime_in_cents: 0,

        active_sponsor_count: 0,
        former_sponsor_count: 0,
        one_time_sponsor_count: 0,

        sponsors: Vec::new(),
        contributions: Vec::new(),
    };

    for provider_ledger in &ledger.providers {
        for contribution in &provider_ledger.contributions {
            let state = states
                .entry((contribution.provider, contribution.key.clone()))
                .or_default();

            if let Some(account) = &contribution.account {
                state.account = Some(account.clone());
            }

            state.contributions.push(contribution.clone());
        }
    }

    for provider_ledger in &ledger.providers {
        for refund in &provider_ledger.refunds {
            let Some(state) = states.get_mut(&(provider_ledger.provider, refund.key.clone()))
            else {
                continue;
            };

            // Refunds are reported against the contributor rather than a single
            // contribution, so they come off the newest matching one first.
            let mut remaining = refund.amount_in_cents;

            for contribution in state
                .contributions
                .iter_mut()
                .rev()
                .filter(|contribution| contribution.recurring == refund.recurring)
            {
                if remaining <= 0 {
                    break;
                }

                let taken = remaining.min(contribution.paid_in_cents);
                contribution.paid_in_cents -= taken;
                remaining -= taken;
            }
        }
    }

    for ((provider, key), state) in states {
        let Some(first_sponsored_at) = state
            .contributions
            .iter()
            .map(|contribution| contribution.start)
            .min()
        else {
            continue;
        };

        let last_activity_at = state
            .contributions
            .iter()
            .map(|contribution| contribution.end.unwrap_or(contribution.start))
            .max()
            .unwrap_or(first_sponsored_at);

        let mut one_time_in_cents = 0;
        let mut recurring_in_cents = 0;
        let mut monthly_in_cents = 0;
        let mut estimated_months_paid = 0;
        let mut recurring_spells = 0;
        let mut active = false;

        for contribution in &state.contributions {
            if contribution.recurring {
                recurring_spells += 1;
                recurring_in_cents += contribution.paid_in_cents;
                estimated_months_paid += contribution.months_paid;

                if contribution.active() {
                    active = true;
                    monthly_in_cents += contribution.monthly_in_cents;
                }
            } else {
                one_time_in_cents += contribution.paid_in_cents;
            }
        }

        let lifetime_in_cents = one_time_in_cents + recurring_in_cents;

        evaluation.monthly_recurring_in_cents += monthly_in_cents;
        evaluation.one_time_in_cents += one_time_in_cents;
        evaluation.recurring_in_cents += recurring_in_cents;
        evaluation.lifetime_in_cents += lifetime_in_cents;

        if active {
            evaluation.active_sponsor_count += 1;
        } else if recurring_spells > 0 {
            evaluation.former_sponsor_count += 1;
        } else {
            evaluation.one_time_sponsor_count += 1;
        }

        evaluation.contributions.extend(state.contributions);

        evaluation.sponsors.push(EvaluatedSponsor {
            provider,
            key,
            account: state.account,

            active,
            recurring_spells,
            monthly_in_cents,

            one_time_in_cents,
            recurring_in_cents,
            lifetime_in_cents,
            estimated_months_paid,

            first_sponsored_at,
            last_activity_at,
        });
    }

    for provider_ledger in &ledger.providers {
        let Some(reported) = provider_ledger.reported_monthly_in_cents else {
            continue;
        };

        let reconstructed: i64 = evaluation
            .sponsors
            .iter()
            .filter(|sponsor| sponsor.provider == provider_ledger.provider)
            .map(|sponsor| sponsor.monthly_in_cents)
            .sum();

        if reported != reconstructed {
            tracing::warn!(
                "reconstructed monthly {} income ({} cents) does not match the provider's own figure ({} cents)",
                provider_ledger.provider.key(),
                reconstructed,
                reported
            );
        }
    }

    evaluation
        .sponsors
        .sort_by_key(|sponsor| std::cmp::Reverse(sponsor.lifetime_in_cents));
    evaluation
        .contributions
        .sort_by_key(|contribution| contribution.start);

    evaluation
}
