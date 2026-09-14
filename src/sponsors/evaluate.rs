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

    pub native_lifetime_in_cents: IndexMap<String, i64>,
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

            let mut remaining = refund.amount_in_cents;
            let mut native_remaining = refund.native_amount_in_cents;

            for contribution in state
                .contributions
                .iter_mut()
                .rev()
                .filter(|contribution| contribution.recurring == refund.recurring)
            {
                if remaining <= 0 && native_remaining <= 0 {
                    break;
                }

                let taken = remaining.min(contribution.paid_in_cents);
                contribution.paid_in_cents -= taken;
                remaining -= taken;

                if contribution.native_currency == refund.native_currency {
                    let taken = native_remaining.min(contribution.native_paid_in_cents);
                    contribution.native_paid_in_cents -= taken;
                    native_remaining -= taken;
                }
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

        let mut native_lifetime_in_cents: IndexMap<String, i64> = IndexMap::new();
        let mut one_time_in_cents = 0;
        let mut recurring_in_cents = 0;
        let mut monthly_in_cents = 0;
        let mut estimated_months_paid = 0;
        let mut recurring_spells = 0;
        let mut active = false;

        for contribution in &state.contributions {
            *native_lifetime_in_cents
                .entry(contribution.native_currency.clone())
                .or_default() += contribution.native_paid_in_cents;

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
        native_lifetime_in_cents.retain(|_, cents| *cents > 0);

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
            native_lifetime_in_cents,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sponsors::{ProviderLedger, Refund};

    fn contribution(
        id: &str,
        paid_in_cents: i64,
        native_currency: &str,
        native_paid_in_cents: i64,
    ) -> Contribution {
        Contribution {
            provider: Provider::OpenCollective,
            id: id.to_string(),
            key: "contributor".to_string(),
            account: None,

            recurring: false,
            interval: None,
            start: chrono::Utc::now(),
            end: Some(chrono::Utc::now()),

            amount_in_cents: paid_in_cents,
            monthly_in_cents: 0,
            paid_in_cents,
            months_paid: 1,

            native_currency: native_currency.to_string(),
            native_amount_in_cents: native_paid_in_cents,
            native_paid_in_cents,
        }
    }

    fn ledger(contributions: Vec<Contribution>, refunds: Vec<Refund>) -> Ledger {
        Ledger {
            currency: "USD".to_string(),
            providers: vec![ProviderLedger {
                provider: Provider::OpenCollective,
                contributions,
                refunds,
                reported_monthly_in_cents: None,
            }],
            failed_providers: Vec::new(),
        }
    }

    #[test]
    fn a_rate_that_moved_between_polls_leaves_the_native_total_alone() {
        // The same €10.00, converted at 1.160 and then at 1.15319.
        let before = evaluate(&ledger(
            vec![contribution("order", 1160, "EUR", 1000)],
            vec![],
        ));
        let after = evaluate(&ledger(
            vec![contribution("order", 1153, "EUR", 1000)],
            vec![],
        ));

        assert_ne!(before.lifetime_in_cents, after.lifetime_in_cents);
        assert_eq!(
            before.sponsors[0].native_lifetime_in_cents,
            after.sponsors[0].native_lifetime_in_cents
        );
        assert_eq!(after.sponsors[0].native_lifetime_in_cents["EUR"], 1000);
    }

    #[test]
    fn each_currency_a_contributor_was_charged_in_is_counted_on_its_own() {
        let evaluation = evaluate(&ledger(
            vec![
                contribution("euros", 1160, "EUR", 1000),
                contribution("dollars", 500, "USD", 500),
            ],
            vec![],
        ));

        let native = &evaluation.sponsors[0].native_lifetime_in_cents;

        assert_eq!(evaluation.lifetime_in_cents, 1660);
        assert_eq!(native["EUR"], 1000);
        assert_eq!(native["USD"], 500);
    }

    #[test]
    fn a_refund_comes_off_the_native_total_as_well_as_the_normalised_one() {
        let evaluation = evaluate(&ledger(
            vec![contribution("order", 1160, "EUR", 1000)],
            vec![Refund {
                key: "contributor".to_string(),
                recurring: false,
                amount_in_cents: 464,

                native_currency: "EUR".to_string(),
                native_amount_in_cents: 400,
            }],
        ));

        assert_eq!(evaluation.lifetime_in_cents, 696);
        assert_eq!(evaluation.sponsors[0].native_lifetime_in_cents["EUR"], 600);
    }

    #[test]
    fn a_refund_only_touches_the_currency_it_was_charged_in() {
        let evaluation = evaluate(&ledger(
            vec![
                contribution("euros", 1160, "EUR", 1000),
                contribution("dollars", 500, "USD", 500),
            ],
            vec![Refund {
                key: "contributor".to_string(),
                recurring: false,
                amount_in_cents: 500,

                native_currency: "USD".to_string(),
                native_amount_in_cents: 500,
            }],
        ));

        let native = &evaluation.sponsors[0].native_lifetime_in_cents;

        assert_eq!(native["EUR"], 1000);
        assert!(!native.contains_key("USD"));
    }
}
