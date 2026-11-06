//! Licence classes (#134): what a Resource's licence lets the user do, for the Get Resources
//! licence filter. SWORD confs give `DistributionLicense` as free text ("Creative Commons:
//! by-nc-nd", "Copyrighted; Permission to distribute granted to CrossWire"); the Open
//! catalogue and the Library give SPDX ids (`CC-BY-SA-4.0`, `LicenseRef-PublicDomain`). Both
//! map to one [`LicenceClass`] through two allowlists of whole values seen in the live
//! catalogs; free text is never read for its meaning. Any other value is
//! [`LicenceClass::Unclear`], which is never shown as free.

use serde::{Deserialize, Serialize};

/// What a licence lets the user do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenceClass {
    /// Public Domain, CC0, CC BY, CC BY-SA or GPL: anyone may share it, changed or not.
    FreeToShare,
    /// Free to use, but sharing is limited: permission to distribute granted to a
    /// repository, non-commercial, or no derivatives.
    PersonalUse,
    /// No licence, or a value on neither allowlist.
    Unclear,
}

impl LicenceClass {
    /// Shown on a row's licence badge hover and in `getbibles.list`.
    pub fn label(self) -> &'static str {
        match self {
            LicenceClass::FreeToShare => "Free to share",
            LicenceClass::PersonalUse => "Free for personal use",
            LicenceClass::Unclear => "Licence unclear",
        }
    }
}

/// Every published licence value that is Free to share, normalized ([`normalized`]): the
/// `DistributionLicense` values seen in the live catalogs of the repositories on CrossWire's
/// master list (2026-11-06; 2,411 confs, 60 distinct values) that grant Public Domain, CC0,
/// CC BY, CC BY-SA or GPL, and the SPDX ids the Starter library and the Open catalogue use.
/// A value is matched whole, never by a part of it.
const FREE_TO_SHARE: &[&str] = &[
    "public domain",
    "public domain.",
    "creative commons: by-sa",
    "creative commons: by-sa 4.0",
    "creative commons: by",
    "creative commons: by 4.0",
    "creative commons: cc0",
    "creative commons attribution 3.0 brazil",
    "gpl",
    "this work is made available under the creative commons attribution-share alike 4.0 \
     international license (cc by-sa).",
    "cc by 4.0 (credit to \"step bible\" linked to www.stepbible.org. report errors to \
     stepbible@gmail.com)",
    "<a href='https://creativecommons.org/licenses/by-sa/4.0/'>cc by-sa 4.0</a>",
    "licenseref-publicdomain",
    "cc0-1.0",
    "cc-by-4.0",
    "cc-by-sa-4.0",
];

/// Every published licence value that is Free for personal use, normalized: the live
/// `DistributionLicense` values (see [`FREE_TO_SHARE`]) that grant use with a limit on
/// sharing (permission to distribute granted to a repository, non-commercial, or no
/// derivatives), and the SPDX ids of those Creative Commons licences.
const PERSONAL_USE: &[&str] = &[
    "creative commons: by-nc-nd",
    "creative commons: by-nc-nd 4.0",
    "creative commons: by-nc-sa",
    "creative commons: by-nc-sa 4.0",
    "creative commons: by-nd",
    "creative commons: by-nd 3.0",
    "copyrighted; permission to distribute granted to ebible.org",
    "copyrighted; permission to distribute granted to crosswire",
    "copyrighted; permission to distribute granted to stepbible",
    "copyrighted; permission to distribute granted to tyndale house, cambridge, uk",
    "copyrighted. permission to distribute granted to crosswire bible society",
    "copyrighted. distribution permitted to crosswire bible society",
    "copyrighted kitab company - kş; permission to distribute granted to crosswire",
    "copyright 2007 united bible societies; permission to distribute granted to crosswire",
    "copyrighted; freely distributable",
    "copyrighted; free non-commercial distribution",
    "copyrighted; permission granted to distribute non-commercially in sword format",
    "copyrighted. noncommercial distribution granted to crosswire. no other distribution \
     permitted.",
    "with permission of the international bible society free for non commercial use only",
    "for non-commercial use only. distribution rights granted to crosswire by ibs (contact for \
     ibs: graham sopp). non-commercial redistribution of this unchanged module is allowed.",
    "creative commons attribution-noncommercial-sharealike 4.0 international license - paul \
     haenze - maps@hopeandfuture.pl",
    "licencja creative commons \"uznanie autorstwa-użycie niekomercyjne-na tych samych \
     warunkach 4.0 międzynarodowe\" (cc by-nc-sa 4.0) - paul haenze - maps@hopeandfuture.pl",
    "cc-by-nc-nd-4.0",
    "cc-by-nc-sa-4.0",
    "cc-by-nd-4.0",
];

/// A licence value as the allowlists hold it: lowercased, with each run of whitespace one
/// space and none at either end. Nothing else is changed, so any other difference (another
/// word, a sentence more, a different hyphen) makes it a different value.
fn normalized(licence: &str) -> String {
    licence
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The class of a licence as published: a conf's `DistributionLicense` or an SPDX id. Only
/// a value on one of the allowlists ([`FREE_TO_SHARE`], [`PERSONAL_USE`]), matched whole
/// after [`normalized`], has a class; every other value, including an allowlisted one with
/// anything added ("Public Domain; no redistribution"), is Unclear and never shown as free.
pub fn licence_class(licence: &str) -> LicenceClass {
    let l = normalized(licence);
    if FREE_TO_SHARE.contains(&l.as_str()) {
        LicenceClass::FreeToShare
    } else if PERSONAL_USE.contains(&l.as_str()) {
        LicenceClass::PersonalUse
    } else {
        LicenceClass::Unclear
    }
}

/// The Get Resources licence filter. Each choice includes the stricter ones: Free for
/// personal use also lists what is Free to share. Unclear licences show only under All.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LicenceFilter {
    #[default]
    All,
    PersonalUse,
    FreeToShare,
}

impl LicenceFilter {
    /// The filter row, in order.
    pub const CHOICES: [LicenceFilter; 3] = [
        LicenceFilter::FreeToShare,
        LicenceFilter::PersonalUse,
        LicenceFilter::All,
    ];

    /// Whether a Resource of licence class `class` passes.
    pub fn accepts(self, class: LicenceClass) -> bool {
        match self {
            LicenceFilter::All => true,
            LicenceFilter::PersonalUse => class != LicenceClass::Unclear,
            LicenceFilter::FreeToShare => class == LicenceClass::FreeToShare,
        }
    }

    /// Shown on the filter row. All is "All licences" there, apart from the type filter's All.
    pub fn label(self) -> &'static str {
        match self {
            LicenceFilter::All => "All licences",
            LicenceFilter::PersonalUse => LicenceClass::PersonalUse.label(),
            LicenceFilter::FreeToShare => LicenceClass::FreeToShare.label(),
        }
    }

    /// What the filter shows, on hover.
    pub fn hover(self) -> &'static str {
        match self {
            LicenceFilter::All => "Every Resource, whatever its licence",
            LicenceFilter::PersonalUse => {
                "Free to share, plus Resources you may use but not pass on freely: \
                 permission to distribute, non-commercial, or no changes allowed"
            }
            LicenceFilter::FreeToShare => {
                "Only Public Domain, CC0, CC BY, CC BY-SA and GPL Resources: anyone may share \
                 them"
            }
        }
    }

    /// The id in commands: `free-to-share`, `personal-use`, `all` (or empty).
    pub fn from_id(id: &str) -> Result<Self, String> {
        match id.trim().to_lowercase().as_str() {
            "" | "all" => Ok(LicenceFilter::All),
            "personal-use" | "free-for-personal-use" => Ok(LicenceFilter::PersonalUse),
            "free-to-share" | "free" => Ok(LicenceFilter::FreeToShare),
            other => Err(format!(
                "Unknown licence filter {other:?}: use free-to-share, personal-use or all"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LicenceClass::{FreeToShare, PersonalUse, Unclear};
    use super::*;

    /// Every distinct `DistributionLicense` in the live catalogs of the repositories on
    /// CrossWire's master list (CrossWire, Beta, Attic, Wycliffe, Bible.org, Xiphos, eBible.org,
    /// Deutsche Bibelgesellschaft, Lockman, IBT, STEP Bible), read on 2026-11-06, with its
    /// class. Values are exact; only two long Unclear ones are cut.
    const LIVE_VALUES: &[(&str, LicenceClass)] = &[
        ("Creative Commons: by-nc-nd", PersonalUse),
        ("Public Domain", FreeToShare),
        ("Creative Commons: by-sa", FreeToShare),
        (
            "Copyrighted; Permission to distribute granted to eBible.org",
            PersonalUse,
        ),
        (
            "Copyrighted; Permission to distribute granted to CrossWire",
            PersonalUse,
        ),
        ("Creative Commons: BY-NC-ND 4.0", PersonalUse),
        ("Copyrighted; Free non-commercial distribution", PersonalUse),
        ("Creative Commons: by-nd", PersonalUse),
        ("Creative Commons: BY-SA 4.0", FreeToShare),
        ("", Unclear),
        ("Copyrighted", Unclear),
        ("GPL", FreeToShare),
        ("Copyrighted; Freely distributable", PersonalUse),
        ("Creative Commons: BY-NC-SA 4.0", PersonalUse),
        ("Creative Commons: BY 4.0", FreeToShare),
        (
            "Copyrighted; Permission granted to distribute non-commercially in SWORD format",
            PersonalUse,
        ),
        ("Creative Commons: CC0", FreeToShare),
        ("See README file", Unclear),
        ("Public domain.", FreeToShare),
        ("Unrestricted", Unclear),
        ("См. файл", Unclear),
        (
            "© Қазақша аудармасы және көркемделуі «Жаңа өмір» баспасы, Стамбул, 1993, 1996, \
             2000, 2010.",
            Unclear,
        ),
        ("none", Unclear),
        ("Permesso di utilisarlo da Richard Wilson.", Unclear),
        ("GFDL", Unclear),
        (
            "FROM www.kretzmannproject.org: \\par\\parThe Popular Commentary is in the public \
             domain. The original volumes were not copyrighted.",
            Unclear,
        ),
        ("Creative Commons: by-nd 3.0", PersonalUse),
        ("Creative Commons: by-nc-sa", PersonalUse),
        ("Creative Commons Attribution 3.0 Brazil", FreeToShare),
        (
            "Copyrighted; Permission to distribute granted to STEPBible",
            PersonalUse,
        ),
        ("Copyrighted.", Unclear),
        ("о. Леонид Лутковский, 1991", Unclear),
        ("Библейская Лига, 1996", Unclear),
        ("public domain", FreeToShare),
        (
            "copyrighted. Permission to distribute granted to CrossWire Bible Society",
            PersonalUse,
        ),
        (
            "With Permission of the International Bible Society free for non commercial use only ",
            PersonalUse,
        ),
        ("Varies.  See hymn description.", Unclear),
        (
            "Used by permission of the International Bible Society",
            Unclear,
        ),
        ("Unknown", Unclear),
        (
            "This work is made available under the Creative Commons Attribution-Share Alike 4.0 \
             International License (CC BY-SA).",
            FreeToShare,
        ),
        ("RBC Ministries, 2000", Unclear),
        ("Public domain", FreeToShare),
        ("Permission de l’utiliser par Jim WANDERSCHEID.", Unclear),
        (
            "Licencja Creative Commons \"Uznanie autorstwa-Użycie niekomercyjne-Na tych samych \
             warunkach 4.0 Międzynarodowe\" (CC BY-NC-SA 4.0) - Paul Haenze - \
             maps@hopeandfuture.pl",
            PersonalUse,
        ),
        ("Hanssler-Verlag, 1998", Unclear),
        ("Gospel Literature Services, 1995", Unclear),
        // Names neither the GNU GPL nor a version.
        (
            "General public license for distribution for any purpose",
            Unclear,
        ),
        (
            "For Non-commercial use only. Distribution rights granted to Crosswire by IBS \
             (Contact for IBS: Graham Sopp). Non-commercial redistribution of this unchanged \
             module is allowed.",
            PersonalUse,
        ),
        ("Creative Commons: by", FreeToShare),
        (
            "Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International License - \
             Paul Haenze - maps@hopeandfuture.pl",
            PersonalUse,
        ),
        (
            "Copyrighted; Permission to distribute granted to Tyndale House, Cambridge, UK",
            PersonalUse,
        ),
        (
            "Copyrighted; Permission to distribute granted to CrossWire ",
            PersonalUse,
        ),
        (
            "Copyrighted. Noncommercial distribution granted to CrossWire. No other distribution \
             permitted.",
            PersonalUse,
        ),
        (
            "Copyrighted. Distribution permitted to CrossWire Bible Society",
            PersonalUse,
        ),
        ("Copyrighted see http://www.bible.org/copyright", Unclear),
        (
            "Copyrighted Kitab Company - KŞ; Permission to distribute granted to CrossWire",
            PersonalUse,
        ),
        (
            "Copyright.  Permission to use granted to STEPBible.",
            Unclear,
        ),
        ("Copyright DBG", Unclear),
        (
            "Copyright 2007 United Bible Societies; Permission to distribute granted to CrossWire",
            PersonalUse,
        ),
        (
            "CC BY 4.0 (Credit to \"STEP Bible\" linked to www.STEPBible.org. Report errors to \
             STEPBible@gmail.com)",
            FreeToShare,
        ),
        ("British and Foreign Bible Society, 1970", Unclear),
        ("Baptist World Alliance, 1986", Unclear),
        (
            "<a href='https://creativecommons.org/licenses/by-sa/4.0/'>CC BY-SA 4.0</a>",
            FreeToShare,
        ),
    ];

    #[test]
    fn every_live_distribution_license_has_its_class() {
        for (value, class) in LIVE_VALUES {
            assert_eq!(licence_class(value), *class, "{value:?}");
        }
    }

    /// The allowlists are the whole rule: each entry is stored normalized, is on one list
    /// only, and classifies as its list; every entry is a live value or an SPDX id.
    #[test]
    fn the_allowlists_classify_exactly_their_entries() {
        for (list, class) in [(FREE_TO_SHARE, FreeToShare), (PERSONAL_USE, PersonalUse)] {
            for entry in list {
                assert_eq!(normalized(entry), *entry, "{entry:?} is not normalized");
                assert_eq!(licence_class(entry), class, "{entry:?}");
                let live = LIVE_VALUES
                    .iter()
                    .any(|(v, c)| normalized(v) == *entry && *c == class);
                let spdx = !entry.contains(' ') && entry.contains('-');
                assert!(
                    live || spdx,
                    "{entry:?} is neither a live value nor an SPDX id"
                );
            }
        }
        for entry in FREE_TO_SHARE {
            assert!(!PERSONAL_USE.contains(entry), "{entry:?} is on both lists");
        }
    }

    #[test]
    fn spdx_ids_classify_like_the_conf_values() {
        for (spdx, class) in [
            ("LicenseRef-PublicDomain", FreeToShare),
            ("CC0-1.0", FreeToShare),
            ("CC-BY-4.0", FreeToShare),
            ("CC-BY-SA-4.0", FreeToShare),
            ("CC-BY-NC-ND-4.0", PersonalUse),
            ("CC-BY-NC-SA-4.0", PersonalUse),
            ("CC-BY-ND-4.0", PersonalUse),
            ("LicenseRef-unknown", Unclear),
            ("MIT", Unclear),
            ("CC-BY-SA-4.0 OR LicenseRef-Proprietary", Unclear),
        ] {
            assert_eq!(licence_class(spdx), class, "{spdx}");
        }
    }

    /// Only case and whitespace are normalized: an allowlisted value in other capitals or
    /// with other spacing keeps its class.
    #[test]
    fn case_and_whitespace_do_not_change_the_class() {
        for value in [
            "PUBLIC DOMAIN",
            "  Public   Domain\n",
            "Public\u{a0}Domain",
            "creative commons:  BY-SA",
        ] {
            assert!(
                LicenceFilter::FreeToShare.accepts(licence_class(value)),
                "{value:?}"
            );
        }
    }

    /// #134 reviews: every value that an earlier, free-text reading of licences let through
    /// or had to special-case is Unclear: negations, qualifications, restrictions however
    /// they are spelled, denials of sharing, exclusions in another sentence, and other
    /// separators. None passes either licence filter.
    #[test]
    fn values_off_the_allowlists_are_unclear_and_never_free() {
        for value in [
            // Unknown values.
            "Ask the publisher",
            "Copyright reserved by the translators",
            "Distributed by the Society",
            "All rights reserved",
            "Creative Commons",
            "Free",
            // Negated or qualified grants.
            "Not Public Domain; all rights reserved",
            "Not Public Domain",
            "This text is not in the public domain.",
            "No longer in the Public Domain",
            "Public Domain? No.",
            "Public Domain, except the notes",
            "Public domain, but the notes are copyrighted",
            "Public Domain. All rights reserved.",
            "Public Domain. All  rights\u{a0}reserved.",
            "Creative Commons: BY-SA 4.0; all rights reserved",
            "Creative Commons: BY-SA 4.0; all\u{2011}rights reserved",
            "Not licensed under Creative Commons: by-sa",
            "Not under the GPL",
            "GPL licence revoked",
            "Copyrighted; no permission to distribute granted to CrossWire",
            "Copyrighted; permission to distribute granted to CrossWire was withdrawn",
            "Not for non-commercial use under CC BY-NC",
            "Copyrighted; distribution not permitted to CrossWire",
            "This Bible is in the Public Domain. It is not copyrighted.",
            // No-derivatives and non-commercial licences, however written.
            "Creative Commons Attribution-NoDerivs 3.0 Unported",
            "Creative Commons Attribution No Derivative Works 2.0",
            "Creative Commons Attribution-NoDerivatives 4.0 International",
            "Creative Commons Attribution-No-Derivs 2.5",
            "Creative Commons Attribution-NoDerivativeWorks 2.0",
            "CC BY-ND 4.0",
            "Attribution-NoDerivs",
            "Public Domain; no derivative works",
            "Creative Commons BY-ND; no derivative works",
            "Creative Commons Attribution Non\u{2011}Commercial 4.0",
            "Creative Commons Attribution Non  Commercial 4.0",
            "Creative Commons Attribution Non\u{a0}Commercial 4.0",
            "Creative Commons Attribution-NonCommercial 4.0",
            "CC BY\u{2011}NC 4.0",
            "Creative Commons: by\u{2011}sa",
            "Creative Commons Attribution 4.0, commercial use prohibited",
            "Public Domain. Not for commercial use.",
            "Public Domain.\nFor non\u{2011}commercial use only.",
            // Denials of sharing.
            "Public Domain; no redistribution",
            "Public Domain. Redistribution prohibited.",
            "Public Domain; you may not share it",
            "Public Domain; not for redistribution",
            "Creative Commons: BY-SA 4.0; do not distribute",
            "CC0-1.0; distribution forbidden",
            "GPL\nReproduction restricted to the publisher",
            "Creative Commons Attribution 4.0; sharing is not permitted",
            "Public Domain; no restrictions on distribution",
            // Licences excluded in another sentence.
            "Creative Commons Attribution Share-Alike license 4.0. This licence does not apply \
             to this edition.",
            "Public Domain. That licence doesn't apply here.",
            "CC BY 4.0\nThe above licence is void.",
            "GPL; this edition is not covered by it",
            "CC0-1.0. Not licensed for this edition.",
            "Copyrighted; Permission to distribute granted to CrossWire. The licence does not \
             apply to the notes.",
            // A live value with anything added.
            "Public Domain..",
            "Creative Commons: by-sa 4.0 (except the notes)",
            "Creative Commons: by-nc-nd; no redistribution",
        ] {
            let class = licence_class(value);
            assert_eq!(class, Unclear, "{value:?}");
            assert!(!LicenceFilter::FreeToShare.accepts(class), "{value:?}");
            assert!(!LicenceFilter::PersonalUse.accepts(class), "{value:?}");
            assert!(LicenceFilter::All.accepts(class), "{value:?}");
        }
    }

    #[test]
    fn each_filter_includes_the_stricter_ones() {
        use LicenceFilter as F;
        let shown = |f: F| -> Vec<LicenceClass> {
            [FreeToShare, PersonalUse, Unclear]
                .into_iter()
                .filter(|c| f.accepts(*c))
                .collect()
        };
        assert_eq!(shown(F::FreeToShare), [FreeToShare]);
        assert_eq!(shown(F::PersonalUse), [FreeToShare, PersonalUse]);
        assert_eq!(shown(F::All), [FreeToShare, PersonalUse, Unclear]);
        assert_eq!(F::from_id("free-to-share"), Ok(F::FreeToShare));
        assert_eq!(F::from_id("Personal-Use"), Ok(F::PersonalUse));
        assert_eq!(F::from_id(""), Ok(F::All));
        assert!(F::from_id("gratis").is_err());
    }
}
