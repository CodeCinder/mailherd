//! Removal of link-tracking parameters, redirect wrappers and tracking pixels.

use url::Url;

/// Query parameters that only exist to track clicks.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "yclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "oly_anon_id",
    "oly_enc_id",
    "vero_id",
    "vero_conv",
    "rb_clickid",
    "s_cid",
    "trk",
    "trkcampaign",
    "sc_campaign",
    "ss_campaign_id",
    "ml_subscriber",
    "ml_subscriber_hash",
    "spm",
    "ref_src",
];

/// Query parameter prefixes that only exist to track clicks.
const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "mtm_", "hsa_"];

/// Hosts that serve open-tracking pixels. Images from these are always removed,
/// even when the user allows remote images.
const TRACKER_HOSTS: &[&str] = &[
    "list-manage.com",
    "mailchimp.com",
    "sendgrid.net",
    "mandrillapp.com",
    "mailgun.org",
    "sparkpostmail.com",
    "hubspotemail.net",
    "hs-analytics.net",
    "exacttarget.com",
    "mcimg.com",
    "rs6.net",
    "mixpanel.com",
    "pixel.wp.com",
    "track.customer.io",
    "mailtrack.io",
    "getnotify.com",
    "bananatag.com",
    "yesware.com",
    "mailstat.us",
];

/// True if `host` belongs to a known open-tracking service.
pub fn is_tracker_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    TRACKER_HOSTS
        .iter()
        .any(|t| host == *t || host.ends_with(&format!(".{t}")))
}

fn is_tracking_param(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    TRACKING_PARAMS.contains(&name.as_str())
        || TRACKING_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// If `url` is a known redirect wrapper, return the URL it points to.
fn unwrap_redirect(url: &Url) -> Option<Url> {
    let host = url.host_str()?.to_ascii_lowercase();
    let param = if host.ends_with("safelinks.protection.outlook.com") {
        "url"
    } else if (host == "www.google.com" || host == "google.com") && url.path() == "/url" {
        "q"
    } else if (host == "l.facebook.com" && url.path() == "/l.php") || host == "l.instagram.com" {
        "u"
    } else {
        return None;
    };
    let target = url.query_pairs().find(|(k, _)| k == param)?.1.into_owned();
    let parsed = Url::parse(&target).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

/// Remove tracking parameters and redirect wrappers from a link.
///
/// Anything that isn't an http(s) URL is returned unchanged.
pub fn clean_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw.trim()) else {
        return raw.to_string();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return raw.to_string();
    }
    // Wrappers can nest (a safelink around a Google redirect), so unwrap a few levels.
    for _ in 0..3 {
        match unwrap_redirect(&url) {
            Some(inner) => url = inner,
            None => break,
        }
    }
    if url.query().is_some() {
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| !is_tracking_param(k))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if kept.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(kept);
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_utm_and_click_ids() {
        assert_eq!(
            clean_url("https://shop.example/item?id=7&utm_source=news&utm_medium=email&fbclid=abc"),
            "https://shop.example/item?id=7"
        );
        assert_eq!(
            clean_url("https://shop.example/?utm_campaign=x"),
            "https://shop.example/"
        );
    }

    #[test]
    fn unwraps_redirect_wrappers() {
        let safelink = "https://eur01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.org%2Fdoc%3Futm_source%3Dx&data=05";
        assert_eq!(clean_url(safelink), "https://example.org/doc");
        let google = "https://www.google.com/url?q=https://example.org/a&sa=D";
        assert_eq!(clean_url(google), "https://example.org/a");
    }

    #[test]
    fn leaves_other_links_alone() {
        assert_eq!(
            clean_url("mailto:ada@example.org"),
            "mailto:ada@example.org"
        );
        assert_eq!(clean_url("not a url"), "not a url");
        assert_eq!(
            clean_url("https://example.org/path?page=2"),
            "https://example.org/path?page=2"
        );
    }

    #[test]
    fn recognises_tracker_hosts() {
        assert!(is_tracker_host("us5.list-manage.com"));
        assert!(is_tracker_host("SENDGRID.NET"));
        assert!(!is_tracker_host("example.org"));
        assert!(!is_tracker_host("notsendgrid.net"));
    }
}
