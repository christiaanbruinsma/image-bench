use gettextrs::{bind_textdomain_codeset, bindtextdomain, gettext, ngettext, setlocale, textdomain, LocaleCategory};

use crate::config::{locale_dir, GETTEXT_PACKAGE};

pub fn init() {
    let _ = setlocale(LocaleCategory::LcAll, "");
    let _ = bindtextdomain(GETTEXT_PACKAGE, locale_dir());
    let _ = bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8");
    let _ = textdomain(GETTEXT_PACKAGE);
}

pub fn tr(message: &str) -> String {
    gettext(message)
}

pub fn trn(singular: &str, plural: &str, n: u32) -> String {
    ngettext(singular, plural, n)
}


pub fn tr_args(message: &str, args: &[(&str, String)]) -> String {
    replace_args(gettext(message), args)
}

pub fn trn_args(singular: &str, plural: &str, n: u32, args: &[(&str, String)]) -> String {
    replace_args(ngettext(singular, plural, n), args)
}

fn replace_args(mut message: String, args: &[(&str, String)]) -> String {
    for (key, value) in args {
        message = message.replace(&format!("{{{key}}}"), value);
    }
    message
}
