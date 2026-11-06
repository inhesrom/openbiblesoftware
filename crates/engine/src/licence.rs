//! Licence classes (#134): what a Resource's licence lets the user do, for the Get Resources
//! licence filter. SWORD confs give `DistributionLicense` as free text ("Creative Commons:
//! by-nc-nd", "Copyrighted; Permission to distribute granted to CrossWire"); the Open
//! catalogue and the Library give SPDX ids (`CC-BY-SA-4.0`, `LicenseRef-PublicDomain`). Both
//! map to one [`LicenceClass`]. A value the rules do not recognise is
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
    /// No licence, or one the rules do not recognise.
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

/// Words that turn a licence statement around or qualify it: "Not Public Domain", "no
/// longer under CC BY", "Public Domain except the notes".
const NEGATIONS: &[&str] = &[
    "not",
    "no",
    "never",
    "nor",
    "neither",
    "without",
    "cannot",
    "isn",
    "aren",
    "wasn",
    "doesn",
    "except",
    "excepting",
    "excluding",
    "unless",
    "but",
    "revoked",
    "withdrawn",
];

/// The words after "no" in a Creative Commons no-derivatives restriction ("No Derivatives",
/// "No Derivative Works", "No-Derivs"): that "no" is the restriction's name, not a negation.
const DERIVATIVE_WORDS: &[&str] = &["derivative", "derivatives", "derivs"];

/// One-word spellings of the no-derivatives restriction ("Attribution-NoDerivs 3.0").
const NO_DERIVATIVES_WORDS: &[&str] = &[
    "noderivs",
    "noderivatives",
    "noderivative",
    "noderivativeworks",
];

/// Whether `words` spell out the Creative Commons no-derivatives restriction (not its "ND"
/// abbreviation, which is one only inside a Creative Commons licence).
fn names_no_derivatives<S: AsRef<str>>(words: &[S]) -> bool {
    words.iter().enumerate().any(|(i, w)| {
        let w = w.as_ref();
        NO_DERIVATIVES_WORDS.contains(&w)
            || (w == "no"
                && words
                    .get(i + 1)
                    .is_some_and(|next| DERIVATIVE_WORDS.contains(&next.as_ref())))
    })
}

/// `words` without the "no" of each no-derivatives restriction name.
fn without_restriction_names(words: &[String]) -> Vec<&str> {
    words
        .iter()
        .enumerate()
        .filter(|(i, w)| {
            !(w.as_str() == "no"
                && words
                    .get(i + 1)
                    .is_some_and(|next| DERIVATIVE_WORDS.contains(&next.as_str())))
        })
        .map(|(_, w)| w.as_str())
        .collect()
}

/// The words of `text`, lowercased: runs of letters and digits.
pub(crate) fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether one of `words` negates or qualifies a licence statement.
pub(crate) fn has_negation<S: AsRef<str>>(words: &[S]) -> bool {
    words.iter().any(|w| NEGATIONS.contains(&w.as_ref()))
}

/// `text` cut into clauses at `;`, line breaks and full stops (a `.` followed by a space or
/// the end, so "4.0" and "ebible.org" stay whole). A `?` or `!` does not end one, so
/// "Public Domain? No." stays one clause.
pub(crate) fn clauses(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next_is_space = chars.peek().is_none_or(|(_, n)| n.is_whitespace());
        let end = matches!(c, ';' | '\n') || (c == '.' && next_is_space);
        if end {
            out.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&text[start..]);
    out
}

/// The class of a licence as published: a conf's `DistributionLicense` or an SPDX id.
/// Restrictions win: a non-commercial or no-derivatives clause is never Free to share. A
/// clause that names a licence and also negates or qualifies it ("Not Public Domain",
/// "CC BY except the notes"), or a free licence beside "all rights reserved", is not a
/// form the rules know, so it is Unclear.
pub fn licence_class(licence: &str) -> LicenceClass {
    let l = licence.to_lowercase();
    let negated_grant = clauses(&l).into_iter().any(|clause| {
        let clause_words = words(clause);
        let kept = without_restriction_names(&clause_words);
        has_negation(&kept) && class_ignoring_negation(&kept.join(" ")) != LicenceClass::Unclear
    });
    if negated_grant {
        return LicenceClass::Unclear;
    }
    if l.contains("all rights reserved") && class_names_free_licence(&l) {
        return LicenceClass::Unclear;
    }
    class_ignoring_negation(&l)
}

/// Whether `l` (lowercased) names Public Domain, a Creative Commons licence or the GPL.
fn class_names_free_licence(l: &str) -> bool {
    let words = words(l);
    let word = |w: &str| words.iter().any(|x| x == w);
    l.contains("public domain")
        || l.contains("creative commons")
        || ["publicdomain", "creativecommons", "cc", "cc0", "gpl"]
            .iter()
            .any(|w| word(w))
}

/// The class of `l` (lowercased) read from its licence names alone.
fn class_ignoring_negation(l: &str) -> LicenceClass {
    let words: Vec<&str> = l
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let word = |w: &str| words.contains(&w);
    let phrase = |p: &str| l.contains(p);
    let creative_commons =
        phrase("creative commons") || word("creativecommons") || word("cc") || word("cc0");
    let non_commercial = word("nc")
        || [
            "non-commercial",
            "noncommercial",
            "non commercial",
            "niekomercyjne",
        ]
        .iter()
        .any(|p| phrase(p));
    // "ND" alone is a no-derivatives clause only inside a Creative Commons licence; the
    // spelled-out names ("NoDerivs", "No Derivative Works") are one wherever they stand.
    let no_derivatives = (creative_commons && word("nd")) || names_no_derivatives(&words);
    if non_commercial || no_derivatives {
        return LicenceClass::PersonalUse;
    }
    let public_domain = phrase("public domain") || word("publicdomain") || word("cc0");
    let cc_by = creative_commons
        && ["by", "sa", "attribution", "atribuição"]
            .iter()
            .any(|w| word(w));
    let gpl = word("gpl") || phrase("general public license");
    if public_domain || cc_by || gpl {
        return LicenceClass::FreeToShare;
    }
    let permission = [
        "permission to distribute",
        "permission granted to distribute",
        "distribution permitted to",
        "distribution rights granted",
        "freely distributable",
    ]
    .iter()
    .any(|p| phrase(p));
    if permission {
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
    /// class. Long values are cut after the part that decides.
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
        ("Unrestricted", Unclear),
        ("See README file", Unclear),
        ("Public domain.", FreeToShare),
        ("", Unclear),
        ("См. файл", Unclear),
        (
            "© Қазақша аудармасы және көркемделуі «Жаңа өмір» баспасы, Стамбул, 1993",
            Unclear,
        ),
        ("none", Unclear),
        ("Permesso di utilisarlo da Richard Wilson.", Unclear),
        ("GFDL", Unclear),
        (
            "FROM www.kretzmannproject.org: \\par\\parThe Popular Commentary is in the public \
             domain. The original volumes were not copyrighted.",
            FreeToShare,
        ),
        ("Creative Commons: by-nd 3.0", PersonalUse),
        ("Creative Commons: by-nc-sa", PersonalUse),
        ("Creative Commons Attribution 3.0 Brazil", FreeToShare),
        (
            "Copyrighted; Permission to distribute granted to STEPBible",
            PersonalUse,
        ),
        (
            " Licença Creative Commons Atribuição 3.0 Brasil",
            FreeToShare,
        ),
        ("о. Леонид Лутковский, 1991", Unclear),
        ("Библейская Лига, 1996", Unclear),
        ("public domain", FreeToShare),
        (
            "copyrighted. Permission to distribute granted to CrossWire Bible Society",
            PersonalUse,
        ),
        (
            "With Permission of the International Bible Society free for non commercial use only",
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
        ("Permission de l’utiliser par Jim WANDERSCHEID.", Unclear),
        (
            "Licencja Creative Commons \"Uznanie autorstwa-Użycie niekomercyjne-Na tych samych \
             warunkach 4.0 Międzynarodowe\" (CC BY-NC-SA 4.0)",
            PersonalUse,
        ),
        ("Hanssler-Verlag, 1998", Unclear),
        ("Gospel Literature Services, 1995", Unclear),
        (
            "General public license for distribution for any purpose",
            FreeToShare,
        ),
        (
            "For Non-commercial use only. Distribution rights granted to Crosswire by IBS",
            PersonalUse,
        ),
        ("Creative Commons: by", FreeToShare),
        (
            "Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International License",
            PersonalUse,
        ),
        (
            "Copyrighted; Permission to distribute granted to Tyndale House, Cambridge, UK",
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
        ("Copyrighted.", Unclear),
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

    #[test]
    fn spdx_ids_classify_like_the_conf_values() {
        for (spdx, class) in [
            ("LicenseRef-PublicDomain", FreeToShare),
            ("CC0-1.0", FreeToShare),
            ("CC-BY-4.0", FreeToShare),
            ("CC-BY-SA-4.0", FreeToShare),
            ("GPL-3.0-or-later", FreeToShare),
            ("CC-BY-NC-4.0", PersonalUse),
            ("CC-BY-ND-4.0", PersonalUse),
            ("CC-BY-NC-SA-4.0", PersonalUse),
            ("LicenseRef-unknown", Unclear),
            ("MIT", Unclear),
        ] {
            assert_eq!(licence_class(spdx), class, "{spdx}");
        }
    }

    /// #134: unknown values are Unclear and never shown as free; "by" outside a Creative
    /// Commons licence is just a word.
    #[test]
    fn unknown_values_are_unclear_and_never_free() {
        for value in [
            "Ask the publisher",
            "Copyright reserved by the translators",
            "Distributed by the Society",
            "All rights reserved",
            "Creative Commons",
        ] {
            assert_eq!(licence_class(value), Unclear, "{value}");
            assert!(!LicenceFilter::FreeToShare.accepts(licence_class(value)));
            assert!(!LicenceFilter::PersonalUse.accepts(licence_class(value)));
            assert!(LicenceFilter::All.accepts(licence_class(value)));
        }
    }

    /// #134 review: a licence name inside a statement that negates or qualifies it is not a
    /// grant. The rules fail closed: such values are Unclear and pass neither licence filter.
    #[test]
    fn negated_or_qualified_licences_are_unclear() {
        for value in [
            "Not Public Domain; all rights reserved",
            "Not Public Domain",
            "This text is not in the public domain.",
            "No longer in the Public Domain",
            "Public Domain? No.",
            "Public Domain, except the notes",
            "Public domain, but the notes are copyrighted",
            "Public Domain. All rights reserved.",
            "Creative Commons: BY-SA 4.0; all rights reserved",
            "Not licensed under Creative Commons: by-sa",
            "Not under the GPL",
            "GPL licence revoked",
            "Copyrighted; no permission to distribute granted to CrossWire",
            "Copyrighted; permission to distribute granted to CrossWire was withdrawn",
            "Not for non-commercial use under CC BY-NC",
            "Copyrighted; distribution not permitted to CrossWire",
        ] {
            assert_eq!(licence_class(value), Unclear, "{value}");
            assert!(!LicenceFilter::FreeToShare.accepts(licence_class(value)));
            assert!(!LicenceFilter::PersonalUse.accepts(licence_class(value)));
        }
        // A restriction stated beside its grant, or a negation outside the licence's own
        // clause, keeps the grant's class.
        for (value, class) in [
            (
                "Creative Commons Attribution No Derivatives 4.0",
                PersonalUse,
            ),
            ("Creative Commons BY-ND; no derivative works", PersonalUse),
            (
                "Copyrighted; all rights reserved; permission to distribute granted to CrossWire",
                PersonalUse,
            ),
            (
                "This Bible is in the Public Domain. It is not copyrighted.",
                FreeToShare,
            ),
        ] {
            assert_eq!(licence_class(value), class, "{value}");
        }
    }

    /// #134 review: every published spelling of the no-derivatives restriction makes a
    /// licence Free for personal use, never Free to share, even beside "Attribution".
    #[test]
    fn no_derivatives_spellings_are_personal_use() {
        for value in [
            "Creative Commons Attribution-NoDerivs 3.0 Unported",
            "Creative Commons Attribution No Derivative Works 2.0",
            "Creative Commons Attribution-NoDerivatives 4.0 International",
            "Creative Commons Attribution-No Derivative Works 3.0",
            "Creative Commons Attribution-No-Derivs 2.5",
            "Creative Commons Attribution No Derivatives 4.0",
            "Creative Commons Attribution-NoDerivativeWorks 2.0",
            "CC BY-ND 4.0",
            "Attribution-NoDerivs",
            "Public Domain; no derivative works",
        ] {
            assert_eq!(licence_class(value), PersonalUse, "{value}");
            assert!(
                !LicenceFilter::FreeToShare.accepts(licence_class(value)),
                "{value}"
            );
            assert!(
                LicenceFilter::PersonalUse.accepts(licence_class(value)),
                "{value}"
            );
        }
        // "Derivative" without the restriction's "no" is not one.
        assert_eq!(
            licence_class("Creative Commons Attribution 4.0; derivative works welcome"),
            FreeToShare
        );
    }

    #[test]
    fn clauses_end_at_sentence_ends_only() {
        assert_eq!(
            clauses("CC BY 4.0 see www.example.org. Not PD; ok"),
            ["CC BY 4.0 see www.example.org", " Not PD", " ok"]
        );
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
