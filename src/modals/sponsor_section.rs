use poise::Modal;
use serenity::{
    all::Attachment,
    small_fixed_array::{FixedArray, FixedString},
};

/// Stands between a moderator and pulling a live section, since revoking takes it off the
/// website at once and hands the sponsor back the rest of what they paid for.
#[derive(Debug, Modal)]
#[name = "Revoke Sponsor Section"]
#[text_display = "This takes the section off the website straight away and gives the sponsor back the days they did not use. They can be put back with the button that follows."]
pub struct SponsorSectionRevokeModal {
    #[name = "Take this section off the website"]
    #[checkbox]
    pub confirm: bool,
}

#[derive(Debug, Modal)]
#[name = "Sponsor Section"]
pub struct SponsorSectionNameModal {
    #[name = "Name"]
    #[placeholder = "The name shown on the website"]
    #[min_length = 1]
    #[max_length = 64]
    pub name: FixedString<u16>,
}

#[derive(Debug, Modal)]
#[name = "Sponsor Section"]
pub struct SponsorSectionDescriptionModal {
    #[name = "Name"]
    #[placeholder = "The name shown on the website"]
    #[min_length = 1]
    #[max_length = 64]
    pub name: FixedString<u16>,
    #[name = "Description"]
    #[placeholder = "A sentence or two about you"]
    #[min_length = 1]
    #[max_length = 512]
    #[paragraph]
    pub description: FixedString<u16>,
}

#[derive(Debug, Modal)]
#[name = "Sponsor Section"]
pub struct SponsorSectionLogoModal {
    #[name = "Name"]
    #[placeholder = "The name shown on the website"]
    #[min_length = 1]
    #[max_length = 64]
    pub name: FixedString<u16>,
    #[name = "Description"]
    #[placeholder = "A sentence or two about you"]
    #[min_length = 1]
    #[max_length = 512]
    #[paragraph]
    pub description: FixedString<u16>,
    #[name = "Logo"]
    #[description = "A png, jpeg, webp or gif of at most 2 MiB. Leave empty to keep the one you have."]
    #[file_upload]
    #[file_types("image")]
    #[min_values = 0]
    #[max_values = 1]
    pub logo: Option<FixedArray<Attachment>>,
}

#[derive(Debug, Modal)]
#[name = "Sponsor Section"]
pub struct SponsorSectionLinkModal {
    #[name = "Name"]
    #[placeholder = "The name shown on the website"]
    #[min_length = 1]
    #[max_length = 64]
    pub name: FixedString<u16>,
    #[name = "Description"]
    #[placeholder = "A sentence or two about you"]
    #[min_length = 1]
    #[max_length = 512]
    #[paragraph]
    pub description: FixedString<u16>,
    #[name = "Logo"]
    #[description = "A png, jpeg, webp or gif of at most 2 MiB. Leave empty to keep the one you have."]
    #[file_upload]
    #[file_types("image")]
    #[min_values = 0]
    #[max_values = 1]
    pub logo: Option<FixedArray<Attachment>>,
    #[name = "Link"]
    #[placeholder = "https://example.com"]
    #[min_length = 8]
    #[max_length = 256]
    pub link: FixedString<u16>,
}

/// One shape for whichever of the four modals the tier opened, so the rest of the flow does
/// not care which fields the sponsor was asked for.
pub struct SponsorSectionInput {
    pub name: String,
    pub description: Option<String>,
    pub logo: Option<Attachment>,
    pub link: Option<String>,
}

impl SponsorSectionInput {
    pub fn parse(
        tier: crate::sponsors::SectionTier,
        data: serenity::all::ModalInteractionData,
    ) -> Self {
        match tier {
            crate::sponsors::SectionTier::Name => {
                let modal = SponsorSectionNameModal::parse(data);

                Self {
                    name: modal.name.into_string(),
                    description: None,
                    logo: None,
                    link: None,
                }
            }
            crate::sponsors::SectionTier::Description => {
                let modal = SponsorSectionDescriptionModal::parse(data);

                Self {
                    name: modal.name.into_string(),
                    description: Some(modal.description.into_string()),
                    logo: None,
                    link: None,
                }
            }
            crate::sponsors::SectionTier::Logo => {
                let modal = SponsorSectionLogoModal::parse(data);

                Self {
                    name: modal.name.into_string(),
                    description: Some(modal.description.into_string()),
                    logo: modal
                        .logo
                        .and_then(|logo| logo.into_vec().into_iter().next()),
                    link: None,
                }
            }
            crate::sponsors::SectionTier::Link => {
                let modal = SponsorSectionLinkModal::parse(data);

                Self {
                    name: modal.name.into_string(),
                    description: Some(modal.description.into_string()),
                    logo: modal
                        .logo
                        .and_then(|logo| logo.into_vec().into_iter().next()),
                    link: Some(modal.link.into_string()),
                }
            }
        }
    }
}

/// The modal for a tier, pre-filled with whatever the member already has so a small change
/// does not mean typing everything again. A logo can never be pre-filled.
pub fn create(
    tier: crate::sponsors::SectionTier,
    section: Option<&crate::models::SponsorSection>,
    custom_id: String,
) -> serenity::all::CreateInteractionResponse<'static> {
    let name = section.map(|section| FixedString::from_str_trunc(&section.name));
    let description = section
        .and_then(|section| section.description.as_deref())
        .map(FixedString::from_str_trunc);
    let link = section
        .and_then(|section| section.link.as_deref())
        .map(FixedString::from_str_trunc);

    match tier {
        crate::sponsors::SectionTier::Name => SponsorSectionNameModal::create(
            name.map(|name| SponsorSectionNameModal { name }),
            custom_id,
        ),
        crate::sponsors::SectionTier::Description => SponsorSectionDescriptionModal::create(
            name.map(|name| SponsorSectionDescriptionModal {
                name,
                description: description.unwrap_or_default(),
            }),
            custom_id,
        ),
        crate::sponsors::SectionTier::Logo => SponsorSectionLogoModal::create(
            name.map(|name| SponsorSectionLogoModal {
                name,
                description: description.unwrap_or_default(),
                logo: None,
            }),
            custom_id,
        ),
        crate::sponsors::SectionTier::Link => SponsorSectionLinkModal::create(
            name.map(|name| SponsorSectionLinkModal {
                name,
                description: description.unwrap_or_default(),
                logo: None,
                link: link.unwrap_or_default(),
            }),
            custom_id,
        ),
    }
}
