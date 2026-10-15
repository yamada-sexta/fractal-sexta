//! Text normalization, escaping, and link detection.
use std::fmt::Write;

use gtk::glib::markup_escape_text;
use linkify::{LinkFinder, LinkKind};
use ruma::{MatrixUri, RoomAliasId, RoomId, UserId};
use url::Url;
const EMAIL_URI_PREFIX: &str = "mailto:";
const HTTPS_URI_PREFIX: &str = "https://";
const MATRIX_URI_SCHEME: &str = "matrix";
#[cfg(test)]
mod tests;
/// Common extensions to strings.
pub trait StrExt {
    /// Escape markup for compatibility with Pango.
    fn escape_markup(&self) -> String;

    /// Collapse contiguous whitespaces in this string into a single space.
    fn collapse_whitespaces(&self, trim_start: bool, trim_end: bool) -> String;
}

impl<T> StrExt for T
where
    T: AsRef<str>,
{
    fn escape_markup(&self) -> String {
        markup_escape_text(self.as_ref()).into()
    }

    fn collapse_whitespaces(&self, trim_start: bool, trim_end: bool) -> String {
        let mut str = self.as_ref();

        if trim_start {
            str = str.trim_start();
        }
        if trim_end {
            str = str.trim_end();
        }

        let mut new_string = String::with_capacity(str.len());
        let mut prev_is_space = false;

        for char in str.chars() {
            if char.is_whitespace() {
                if prev_is_space {
                    // We have already added a space as the last character, ignore this whitespace.
                    continue;
                }

                prev_is_space = true;
                new_string.push(' ');
            } else {
                prev_is_space = false;
                new_string.push(char);
            }
        }

        new_string
    }
}

/// Common extensions to mutable strings.
pub trait StrMutExt {
    /// Truncate this string at the first newline.
    ///
    /// Appends an ellipsis if the string was truncated.
    ///
    /// Returns `true` if the string was truncated.
    fn truncate_newline(&mut self) -> bool;

    /// Truncate whitespaces at the end of the string.
    fn truncate_end_whitespaces(&mut self);

    /// Append an ellipsis, except if this string already ends with an ellipsis.
    fn append_ellipsis(&mut self);

    /// Strip the NUL byte.
    ///
    /// Since they are used by GTK as the end of a string, strings in properties
    /// will be truncated at the first NUL byte.
    fn strip_nul(&mut self);

    /// Remove unnecessary or problematic characters from the string.
    fn clean_string(&mut self) {
        self.strip_nul();
        self.truncate_end_whitespaces();
    }
}

impl StrMutExt for String {
    fn truncate_newline(&mut self) -> bool {
        let newline = self.find('\n');

        if let Some(newline) = newline {
            self.truncate(newline);
            self.append_ellipsis();
        }

        newline.is_some()
    }

    fn truncate_end_whitespaces(&mut self) {
        if self.is_empty() {
            return;
        }

        let new_len = self
            .char_indices()
            .rfind(|(_, c)| !c.is_whitespace())
            .map(|(idx, c)| {
                // We have the position of the last non-whitespace character, so the last
                // whitespace character is the character after it.
                idx + c.len_utf8()
            })
            // 0 means that there are only whitespaces in the string.
            .unwrap_or_default();

        self.truncate(new_len);
    }

    fn append_ellipsis(&mut self) {
        if !self.ends_with('…') && !self.ends_with("..") {
            self.push('…');
        }
    }

    fn strip_nul(&mut self) {
        self.retain(|c| c != '\0');
    }
}

/// Extensions to `Option<String>`.
pub trait OptionStringExt: Sized {
    /// Remove unnecessary or problematic characters from the string.
    ///
    /// If the final string is empty, replaces it with `None`.
    fn clean_string(&mut self);

    /// Remove unnecessary or problematic characters from the string.
    ///
    /// If the final string is empty, replaces it with `None`.
    fn into_clean_string(mut self) -> Self {
        self.clean_string();
        self
    }
}

impl OptionStringExt for Option<String> {
    fn clean_string(&mut self) {
        self.take_if(|s| {
            s.clean_string();
            s.is_empty()
        });
    }
}

pub trait PangoStrMutExt {
    /// Append the opening Pango markup link tag of the given URI parts.
    ///
    /// The URI is also used as a title, so users can preview the link on hover.
    fn append_link_opening_tag(&mut self, uri: impl AsRef<str>);

    /// Append the given emote's sender name and consumes it, if it is set.
    fn maybe_append_emote_name(&mut self, name: &mut Option<&str>);
}

impl PangoStrMutExt for String {
    fn append_link_opening_tag(&mut self, uri: impl AsRef<str>) {
        let uri = uri.escape_markup();
        // We need to escape the title twice because GTK doesn't take care of it.
        let title = uri.escape_markup();

        let _ = write!(self, r#"<a href="{uri}" title="{title}">"#);
    }

    fn maybe_append_emote_name(&mut self, name: &mut Option<&str>) {
        if let Some(name) = name.take() {
            let _ = write!(self, "<b>{}</b> ", name.escape_markup());
        }
    }
}

/// The textual representation of a room mention.
pub const AT_ROOM: &str = "@room";

/// Find `@room` in the given string.
///
/// This uses the same algorithm as the pushrules from the Matrix spec to detect
/// it in the `body`.
///
/// Returns the position of the first match.
pub fn find_at_room(s: &str) -> Option<usize> {
    for (pos, _) in s.match_indices(AT_ROOM) {
        let is_at_word_start = pos == 0 || s[..pos].ends_with(char_is_ascii_word_boundary);
        if !is_at_word_start {
            continue;
        }

        let pos_after_match = pos + 5;
        let is_at_word_end = pos_after_match == s.len()
            || s[pos_after_match..].starts_with(char_is_ascii_word_boundary);
        if is_at_word_end {
            return Some(pos);
        }
    }

    None
}

/// Whether the given `char` is a word boundary, according to the Matrix spec.
///
/// A word boundary is any character not in the sets `[A-Z]`, `[a-z]`, `[0-9]`
/// or `_`.
fn char_is_ascii_word_boundary(c: char) -> bool {
    !c.is_ascii_alphanumeric() && c != '_'
}

/// Linkify the given text.
///
/// The text will also be escaped with [`StrExt::escape_markup()`].
pub fn linkify(text: &str) -> String {
    let mut linkified = String::with_capacity(text.len());
    Linkifier::new(&mut linkified).linkify(text);
    linkified
}

/// A helper type to linkify text.
pub struct Linkifier<'a> {
    /// The string containing the result.
    inner: &'a mut String,
    /// The mentions detection setting and results.
    mentions: Option<Box<dyn FnMut(&str) -> bool + 'a>>,
    detect_at_room: bool,
    placeholder: &'static str,
}

impl<'a> Linkifier<'a> {
    /// Construct a new linkifier that will add text in the given string.
    pub fn new(inner: &'a mut String) -> Self {
        Self {
            inner,
            mentions: None,
            detect_at_room: false,
            placeholder: "",
        }
    }

    /// Supply mention rendering without depending on UI model or widget types.
    pub fn with_mentions(
        mut self,
        placeholder: &'static str,
        detect_at_room: bool,
        render: impl FnMut(&str) -> bool + 'a,
    ) -> Self {
        self.mentions = Some(Box::new(render));
        self.detect_at_room = detect_at_room;
        self.placeholder = placeholder;
        self
    }

    /// Search and replace links in the given text.
    ///
    /// Returns the list of mentions, if any where found.
    pub fn linkify(mut self, text: &str) {
        let mut finder = LinkFinder::new();
        // Allow URLS without a scheme.
        finder.url_must_have_scheme(false);

        let mut prev_span = None;

        for span in finder.spans(text) {
            let span_text = span.as_str();

            match span.kind() {
                Some(LinkKind::Url) => {
                    let is_valid_url = self.append_detected_url(span_text, prev_span);

                    if is_valid_url {
                        prev_span = None;
                    } else {
                        prev_span = Some(span_text);
                    }
                }
                Some(LinkKind::Email) => {
                    self.inner
                        .append_link_opening_tag(format!("{EMAIL_URI_PREFIX}{span_text}"));
                    self.inner.push_str(&span_text.escape_markup());
                    self.inner.push_str("</a>");

                    // The span was a valid email so we will not need to check it for the next span.
                    prev_span = None;
                }
                _ => {
                    if self.detect_at_room
                        && let Some(pos) = find_at_room(span_text)
                    {
                        self.append_string(&span_text[..pos]);
                        if self.mentions.as_mut().is_some_and(|render| render(AT_ROOM)) {
                            self.inner.push_str(self.placeholder);
                        } else {
                            self.append_string(AT_ROOM);
                        }
                        self.append_string(&span_text[pos + AT_ROOM.len()..]);
                        prev_span = Some(span_text);
                        continue;
                    }

                    self.append_string(span_text);
                    prev_span = Some(span_text);
                }
            }
        }
    }

    /// Append the given string.
    ///
    /// Escapes the markup of the string.
    fn append_string(&mut self, s: &str) {
        self.inner.push_str(&s.escape_markup());
    }

    /// Append the given URI with the given link content.
    fn append_uri(&mut self, uri: &str, content: &str) {
        if self.mentions.as_mut().is_some_and(|render| render(uri)) {
            self.inner.push_str(self.placeholder);
            return;
        }

        self.inner.append_link_opening_tag(uri);
        self.append_string(content);
        self.inner.push_str("</a>");
    }

    /// Append the given string detected as a URL.
    ///
    /// Appends false positives as normal strings, otherwise appends it as a
    /// URI.
    ///
    /// Returns `true` if it was detected as a valid URL.
    fn append_detected_url(&mut self, detected_url: &str, prev_span: Option<&str>) -> bool {
        if Url::parse(detected_url).is_ok() {
            // This is a full URL with a scheme, we can trust that it is valid.
            self.append_uri(detected_url, detected_url);
            return true;
        }

        // It does not have a scheme, try to split it to get only the domain.
        let domain = if let Some((domain, _)) = detected_url.split_once('/') {
            // This is a URL with a path component.
            domain
        } else if let Some((domain, _)) = detected_url.split_once('?') {
            // This is a URL with a query component.
            domain
        } else if let Some((domain, _)) = detected_url.split_once('#') {
            // This is a URL with a fragment.
            domain
        } else {
            // It should only contain the full domain.
            detected_url
        };

        // Check that the top-level domain is known.
        if !domain.rsplit_once('.').is_some_and(|(_, d)| tld::exist(d)) {
            // This is a false positive, treat it like a regular string.
            self.append_string(detected_url);
            return false;
        }

        // The LinkFinder detects the homeserver part of `matrix:` URIs and Matrix
        // identifiers, e.g. it detects `example.org` in `matrix:r/somewhere:
        // example.org` or in `#somewhere:matrix.org`. We can use that to detect the
        // full URI or identifier with the previous span.

        // First, detect if the previous character is `:`, this is common to URIs and
        // identifiers.
        if let Some(prev_span) = prev_span.filter(|s| s.ends_with(':')) {
            // Most identifiers in Matrix do not have a list of allowed characters, so all
            // characters are allowed… which makes it difficult to find where they start.
            // We have to set arbitrary rules for the localpart to match most cases:
            // - No whitespaces
            // - No `:`, as it is the separator between localpart and server name, and after
            //   the scheme in URIs
            // - As soon as we encounter a known sigil, we assume we have the full ID. We
            //   ignore event IDs because we need a room to be able to generate a link.
            if let Some((pos, c)) = prev_span[..]
                .char_indices()
                .rev()
                // Skip the `:` we detected earlier.
                .skip(1)
                .find(|(_, c)| c.is_whitespace() || matches!(c, ':' | '!' | '#' | '@'))
            {
                let maybe_id_start = &prev_span[pos..];

                match c {
                    ':' if prev_span[..pos].ends_with(MATRIX_URI_SCHEME) => {
                        // This should be a matrix URI.
                        let maybe_full_uri =
                            format!("{MATRIX_URI_SCHEME}{maybe_id_start}{detected_url}");
                        if MatrixUri::parse(&maybe_full_uri).is_ok() {
                            // Remove the start of the URI from the string.
                            self.inner.truncate(
                                self.inner.len() - maybe_id_start.len() - MATRIX_URI_SCHEME.len(),
                            );
                            self.append_uri(&maybe_full_uri, &maybe_full_uri);

                            return true;
                        }
                    }
                    '!' => {
                        // This should be a room ID.
                        if let Ok(room_id) =
                            RoomId::parse(format!("{maybe_id_start}{detected_url}"))
                        {
                            // Remove the start of the ID from the string.
                            self.inner.truncate(self.inner.len() - maybe_id_start.len());
                            // Transform it into a link.
                            self.append_uri(&room_id.matrix_to_uri().to_string(), room_id.as_str());
                            return true;
                        }
                    }
                    '#' => {
                        // This should be a room alias.
                        if let Ok(room_alias) =
                            RoomAliasId::parse(format!("{maybe_id_start}{detected_url}"))
                        {
                            // Remove the start of the ID from the string.
                            self.inner.truncate(self.inner.len() - maybe_id_start.len());
                            // Transform it into a link.
                            self.append_uri(
                                &room_alias.matrix_to_uri().to_string(),
                                room_alias.as_str(),
                            );
                            return true;
                        }
                    }
                    '@' => {
                        // This should be a user ID.
                        if let Ok(user_id) =
                            UserId::parse(format!("{maybe_id_start}{detected_url}"))
                        {
                            // Remove the start of the ID from the string.
                            self.inner.truncate(self.inner.len() - maybe_id_start.len());
                            // Transform it into a link.
                            self.append_uri(&user_id.matrix_to_uri().to_string(), user_id.as_str());
                            return true;
                        }
                    }
                    _ => {
                        // We reached a whitespace without a sigil or URI
                        // scheme, this must be a regular URL.
                    }
                }
            }
        }

        self.append_uri(&format!("{HTTPS_URI_PREFIX}{detected_url}"), detected_url);
        true
    }
}
