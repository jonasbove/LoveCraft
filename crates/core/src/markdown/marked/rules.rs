//! The regular expressions of marked 18.1.1 (`src/rules.ts`), as marked compiles them for its
//! default GFM options: generated from the compiled `RegExp` sources, not retyped. Each is
//! compiled once, on first use. The `g` flag is dropped: whether a use is global is decided
//! where it is used, as in marked. Rules only marked's pedantic mode or its own rule
//! building use are left out (`del`, `caret`, `listReplaceNesting`, `pedanticHrefTitle`,
//! `spaceLine`), and two are plain string operations here (`percentDecode`, `splitPipe`).

use crate::js::regexp::Re;
use std::sync::OnceLock;

macro_rules! rule {
    ($name:ident, $source:expr, $flags:expr) => {
        pub(super) fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, $flags))
        }
    };
}

rule!(
    block_blockquote,
    r####"^( {0,3}> ?(([^\n]+(?:\n(?! {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)| {0,3}#{1,6}(?:\s|$)| {0,3}>| {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)| {0,3}(?:[*+-]|\d{1,9}[.)])(?:[ \t]|\n|$)| {0,3}(?:<\/?(?:address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|\n|\/?>)|<(?:(?:script|pre|style|textarea)(?:[\s>]|$)|!--)|<\?|<![A-Za-z]|<!\[CDATA\[)|[ \t]+\n)[^\n]+)*)|[^\n]*)(?:\n|$))+"####,
    ""
);
rule!(
    block_code,
    r####"^((?: {4}| {0,3}\t)[^\n]+(?:\n(?:[ \t]*(?:\n|$))*)?)+"####,
    ""
);
rule!(
    block_def,
    r####"^ {0,3}\[((?!\s*\])(?:\\[\s\S]|[^\[\]\\])+)\]: *(?:\n[ \t]*)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n[ \t]*)?| *\n[ \t]*)((?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))))? *(?:\n+|$)"####,
    ""
);
rule!(
    block_fences,
    r####"^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)"####,
    ""
);
rule!(
    block_heading,
    r####"^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)"####,
    ""
);
rule!(
    block_hr,
    r####"^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)"####,
    ""
);
rule!(
    block_html,
    r####"^ {0,3}(?:<(script|pre|style|textarea)[\s>][\s\S]*?(?:<\/\1>[^\n]*\n*|$)|<!--(?:-?>|[\s\S]*?(?:-->|$))[^\n]*(\n+|$)|<\?[\s\S]*?(?:\?>[^\n]*\n*|$)|<![A-Z][\s\S]*?(?:>[^\n]*\n*|$)|<!\[CDATA\[[\s\S]*?(?:\]\]>[^\n]*\n*|$)|<\/?(address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|\n|\/?>)[\s\S]*?(?:(?:\n[ 	]*)+\n|$)|<(?!(?:script|pre|style|textarea)(?![a-z0-9-]))([a-z][a-z0-9-]*)(?: +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?)*? *\/?>(?=[ \t]*(?:\n|$))[\s\S]*?(?:(?:\n[ 	]*)+\n|$)|<\/[a-z][a-z0-9-]*\s*>(?=[ \t]*(?:\n|$))[\s\S]*?(?:(?:\n[ 	]*)+\n|$))"####,
    "i"
);
rule!(
    block_lheading,
    r####"^(?! {0,3}(?:[*+-]|\d{1,9}[.)]) |(?: {4}| {0,3}\t)| {0,3}(?:`{3,}|~{3,})| {0,3}>| {0,3}#{1,6}(?:\s|$)| {0,3}<[^\n>]+>\n| {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n)((?:.|\n(?!\s*?\n| {0,3}(?:[*+-]|\d{1,9}[.)]) | {0,3}(?:`{3,}|~{3,})| {0,3}>| {0,3}#{1,6}(?:\s|$)| {0,3}(?:(?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)| {0,3}<[^\n>]+>\n| {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n))+?)\n {0,3}(=+|-+) *(?:\n+|$)"####,
    ""
);
rule!(
    block_list,
    r####"^( {0,3}(?:[*+-]|\d{1,9}[.)]))([ \t][^\n]*?)?(?:\n|$)"####,
    ""
);
rule!(block_newline, r####"^(?:[ \t]*(?:\n|$))+"####, "");
rule!(
    block_paragraph,
    r####"^([^\n]+(?:\n(?! {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)| {0,3}#{1,6}(?:\s|$)| {0,3}>| {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)| {0,3}(?:[*+-]|1[.)])[ \t]+[^ \t\n]| {0,3}(?:<\/?(?:address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|\n|\/?>)|<(?:(?:script|pre|style|textarea)(?:[\s>]|$)|!--)|<\?|<![A-Za-z]|<!\[CDATA\[)| *([^\n ].*)\n {0,3}((?:\| *)?:?-+:? *(?:\| *:?-+:? *)*(?:\| *)?)(?:\n((?:(?! *\n| {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)| {0,3}#{1,6}(?:\s|$)| {0,3}>|(?: {4}| {0,3}	)[^\n]| {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)| {0,3}(?:[*+-]|1[.)])[ \t]| {0,3}(?:<\/?(?:address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|\n|\/?>)|<(?:(?:script|pre|style|textarea)(?:[\s>]|$)|!--)|<\?|<![A-Za-z]|<!\[CDATA\[)).*(?:\n|$))*)\n*|$)|[ \t]+\n)[^\n]+)*)"####,
    ""
);
rule!(
    block_table,
    r####"^ *([^\n ].*)\n {0,3}((?:\| *)?:?-+:? *(?:\| *:?-+:? *)*(?:\| *)?)(?:\n((?:(?! *\n| {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)| {0,3}#{1,6}(?:\s|$)| {0,3}>|(?: {4}| {0,3}	)[^\n]| {0,3}(?:`{3,}(?=[^`\n]*(?:\n|$))|~~~)[^\n]*(?:\n|$)| {0,3}(?:[*+-]|1[.)])[ \t]| {0,3}(?:<\/?(?:address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|\n|\/?>)|<(?:(?:script|pre|style|textarea)(?:[\s>]|$)|!--)|<\?|<![A-Za-z]|<!\[CDATA\[)).*(?:\n|$))*)\n*|$)"####,
    ""
);
rule!(block_text, r####"^[^\n]+"####, "");
rule!(
    inline_backpedal,
    r####"(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+"####,
    ""
);
rule!(inline_any_punctuation, r####"\\([\p{P}\p{S}])"####, "u");
rule!(
    inline_any_punctuation_outside_code,
    r####"(`+)(?:[^`]|[^`][\s\S]*?[^`])\1(?!`)|`+|\\([\p{P}\p{S}])"####,
    "u"
);
rule!(
    inline_autolink,
    r####"^<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^\s\x00-\x1f<>]*|[a-zA-Z0-9.!#$%&'*+/=?_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_]))>"####,
    ""
);
rule!(
    inline_block_skip,
    r####"\[(?:[^\[\]`]|(?<a>`+)[^`]+\k<a>(?!`))*?\]\((?:\\[\s\S]|[^\\\(\)]|\((?:\\[\s\S]|[^\\\(\)])*\))*\)|(?<!`)()(?<b>`+)[^`]+\k<b>(?!`)|<(?! )[^<>]*?>"####,
    ""
);
rule!(inline_br, r####"^( {2,}|\\)\n(?!\s*$)[ \t]*"####, "");
rule!(
    inline_code,
    r####"^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)"####,
    ""
);
rule!(
    inline_del_l_delim,
    r####"^~~?(?:((?!~)[\p{P}\p{S}])|[^\s~])"####,
    "u"
);
rule!(
    inline_del_r_delim,
    r####"^[^~]+(?=[^~])|(?!~)[\p{P}\p{S}](~~?)(?=[\s]|$)|[^\s\p{P}\p{S}](~~?)(?!~)(?=[\s\p{P}\p{S}]|$)|(?!~)[\s\p{P}\p{S}](~~?)(?=[^\s\p{P}\p{S}])|[\s](~~?)(?!~)(?=[\p{P}\p{S}])|(?!~)[\p{P}\p{S}](~~?)(?!~)(?=[\p{P}\p{S}])|[^\s\p{P}\p{S}](~~?)(?=[^\s\p{P}\p{S}])"####,
    "u"
);
rule!(
    inline_em_strong_l_delim,
    r####"^(?:\*+(?:((?!\*)(?!~)[\p{P}\p{S}])|([^\s*]))?)|^_+(?:((?!_)(?!~)[\p{P}\p{S}])|([^\s_]))?"####,
    "u"
);
rule!(
    inline_em_strong_r_delim_ast,
    r####"^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)|[^*]+(?=[^*])|(?!\*)(?!~)[\p{P}\p{S}](\*+)(?=[\s]|$)|(?:[^\s\p{P}\p{S}]|~)(\*+)(?!\*)(?=(?!~)[\s\p{P}\p{S}]|$)|(?!\*)(?!~)[\s\p{P}\p{S}](\*+)(?=(?:[^\s\p{P}\p{S}]|~))|[\s](\*+)(?!\*)(?=(?!~)[\p{P}\p{S}])|(?!\*)(?!~)[\p{P}\p{S}](\*+)(?!\*)(?=(?!~)[\p{P}\p{S}])|(?:[^\s\p{P}\p{S}]|~)(\*+)(?=(?:[^\s\p{P}\p{S}]|~))"####,
    "u"
);
rule!(
    inline_em_strong_r_delim_und,
    r####"^[^_*]*?\*\*[^_*]*?_[^_*]*?(?=\*\*)|[^_]+(?=[^_])|(?!_)[\p{P}\p{S}](_+)(?=[\s]|$)|[^\s\p{P}\p{S}](_+)(?!_)(?=[\s\p{P}\p{S}]|$)|(?!_)[\s\p{P}\p{S}](_+)(?=[^\s\p{P}\p{S}])|[\s](_+)(?!_)(?=[\p{P}\p{S}])|(?!_)[\p{P}\p{S}](_+)(?!_)(?=[\p{P}\p{S}])"####,
    "u"
);
rule!(
    inline_escape,
    r####"^\\([!"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])"####,
    ""
);
rule!(
    inline_link,
    r####"^!?\[((?:\[(?:\[(?:\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\])|[^\[\]\\`])*?)\]\([ \t\n]*(<(?:\\.|[^\n<>\\])+>|[^ \t\n\x00-\x1f]+|(?=\)))(?:(?:[ \t]+(?:\n[ \t]*)?|\n[ \t]*)("(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)))?[ \t\n]*\)"####,
    ""
);
rule!(
    inline_nolink,
    r####"^!?\[((?!\s*\])(?:\\[\s\S]|[^\[\]\\])+)\](?:\[\])?"####,
    ""
);
rule!(
    inline_punctuation,
    r####"^((?![*_])[\s\p{P}\p{S}])"####,
    "u"
);
rule!(
    inline_reflink,
    r####"^!?\[((?:\[(?:\[(?:\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\])|[^\[\]\\`])*?)\]\[((?!\s*\])(?:\\[\s\S]|[^\[\]\\])+)\]"####,
    ""
);
rule!(
    inline_reflink_search,
    r####"!?\[((?:[^\[\]\\`]*(?:\[(?:\[(?:\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\]))){0,999}?[^\[\]\\`]*?)\]\[((?!\s*\])(?:\\[\s\S]|[^\[\]\\]){1,999})\]|!?\[((?!\s*\])(?:\\[\s\S]|[^\[\]\\]){1,999})\](?:\[\])?(?!\()"####,
    ""
);
rule!(
    inline_tag,
    r####"^<!--(?:-?>|[\s\S]*?-->)|^<\/[a-zA-Z][a-zA-Z0-9-]*\s*>|^<[a-zA-Z][a-zA-Z0-9-]*(?:\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?)*?\s*\/?>|^<\?[\s\S]*?\?>|^<![a-zA-Z]+\s[\s\S]*?>|^<!\[CDATA\[[\s\S]*?\]\]>"####,
    ""
);
rule!(
    inline_text,
    r####"^(?:[^a-zA-Z0-9](?=(?:mailto|xmpp):)|(`+|~+|[^`~])(?:(?=[`~])|(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|[hH][tT][tT][pP][sS]?|[fF][tT][pP]:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9](?=(?:mailto|xmpp):)|[^a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-](?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@))))"####,
    ""
);
rule!(
    inline_url,
    r####"^(?:mailto:[A-Za-z0-9._+-]+@[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![\w-])|xmpp:[A-Za-z0-9._+-]+@[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![\w-])(?:\/[A-Za-z0-9@.]+)?)|^((?:[hH][tT][tT][pP][sS]?|[fF][tT][pP]):\/\/|www\.)(?:[a-zA-Z0-9\-]+\.?)+[^\s<]*|^[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![\w-])"####,
    ""
);
rule!(
    other_code_remove_indent,
    r####"^(?: {0,3}\t| {1,4})"####,
    "m"
);
rule!(other_output_link_replace, r####"\\([\[\]])"####, "");
rule!(other_indent_code_compensation, r####"^(\s+)(?:```)"####, "");
rule!(other_beginning_space, r####"^\s+"####, "");
rule!(other_ending_hash, r####"#$"####, "");
rule!(other_starting_space_char, r####"^ "####, "");
rule!(other_ending_space_char, r####" $"####, "");
rule!(other_ending_space_tab_char, r####"[ \t]$"####, "");
rule!(other_non_space_char, r####"[^ ]"####, "");
rule!(other_new_line_char_global, r####"\n"####, "");
rule!(other_tab_char_global, r####"\t"####, "");
rule!(other_leading_space_tab, r####"^[ \t]+"####, "");
rule!(other_multiple_space_global, r####"\s+"####, "");
rule!(other_blank_line, r####"^[ \t]*$"####, "");
rule!(other_double_blank_line, r####"\n[ \t]*\n[ \t]*$"####, "");
rule!(other_blockquote_start, r####"^ {0,3}>"####, "");
rule!(
    other_blockquote_setext_replace,
    r####"\n {0,3}((?:=+|-+) *)(?=\n|$)"####,
    ""
);
rule!(
    other_blockquote_setext_replace2,
    r####"^ {0,3}>[ \t]?"####,
    "m"
);
rule!(other_list_is_task, r####"^\[[ xX]\] +\S"####, "");
rule!(other_list_replace_task, r####"^\[[ xX]\] +"####, "");
rule!(other_list_task_checkbox, r####"\[[ xX]\]"####, "");
rule!(other_any_line, r####"\n.*\n"####, "");
rule!(other_href_brackets, r####"^<(.*)>$"####, "");
rule!(other_table_delimiter, r####"[:|]"####, "");
rule!(other_table_align_chars, r####"^\||\| *$"####, "");
rule!(other_table_row_blank_line, r####"\n[ \t]*$"####, "");
rule!(other_table_align_right, r####"^ *-+: *$"####, "");
rule!(other_table_align_center, r####"^ *:-+: *$"####, "");
rule!(other_table_align_left, r####"^ *:-+ *$"####, "");
rule!(other_start_a_tag, r####"^<a "####, "i");
rule!(other_end_a_tag, r####"^<\/a>"####, "i");
rule!(
    other_start_pre_script_tag,
    r####"^<(pre|code|kbd|script)(\s|>)"####,
    "i"
);
rule!(
    other_end_pre_script_tag,
    r####"^<\/(pre|code|kbd|script)(\s|>)"####,
    "i"
);
rule!(other_start_angle_bracket, r####"^<"####, "");
rule!(other_end_angle_bracket, r####">$"####, "");
rule!(other_unicode_alpha_numeric, r####"[\p{L}\p{N}]"####, "u");
rule!(
    other_numeric_character_reference,
    r####"&#(?:(\d{1,7})|[Xx]([A-Fa-f0-9]{1,6}));"####,
    ""
);
rule!(
    other_numeric_character_reference_or_ampersand,
    r####"&#(?:(\d{1,7})|[Xx]([A-Fa-f0-9]{1,6}));|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)"####,
    ""
);
rule!(other_escape_test, r####"[&<>"']"####, "");
rule!(other_escape_replace, r####"[&<>"']"####, "");
rule!(
    other_escape_test_no_encode,
    r####"[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)"####,
    ""
);
rule!(
    other_escape_replace_no_encode,
    r####"[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)"####,
    ""
);
rule!(other_find_pipe, r####"\|"####, "");
rule!(other_slash_pipe, r####"\\\|"####, "");
rule!(other_carriage_return, r####"\r\n|\r"####, "");
rule!(other_not_space_start, r####"^\S*"####, "");
rule!(other_ending_newline, r####"\n$"####, "");

/// `htmlBeginRegex` after its `^ {0,k}` indent.
pub(super) const HTML_BEGIN_TAIL: &str = r####"(?:<\/?(?:address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|$|\/?>)|<(?:(?:script|pre|style|textarea)(?:[\s>]|$)|!--)|<\?|<![A-Za-z]|<!\[CDATA\[)"####;

#[cfg(test)]
pub(super) const ALL: &[fn() -> &'static Re] = &[
    block_blockquote,
    block_code,
    block_def,
    block_fences,
    block_heading,
    block_hr,
    block_html,
    block_lheading,
    block_list,
    block_newline,
    block_paragraph,
    block_table,
    block_text,
    inline_backpedal,
    inline_any_punctuation,
    inline_any_punctuation_outside_code,
    inline_autolink,
    inline_block_skip,
    inline_br,
    inline_code,
    inline_del_l_delim,
    inline_del_r_delim,
    inline_em_strong_l_delim,
    inline_em_strong_r_delim_ast,
    inline_em_strong_r_delim_und,
    inline_escape,
    inline_link,
    inline_nolink,
    inline_punctuation,
    inline_reflink,
    inline_reflink_search,
    inline_tag,
    inline_text,
    inline_url,
    other_code_remove_indent,
    other_output_link_replace,
    other_indent_code_compensation,
    other_beginning_space,
    other_ending_hash,
    other_starting_space_char,
    other_ending_space_char,
    other_ending_space_tab_char,
    other_non_space_char,
    other_new_line_char_global,
    other_tab_char_global,
    other_leading_space_tab,
    other_multiple_space_global,
    other_blank_line,
    other_double_blank_line,
    other_blockquote_start,
    other_blockquote_setext_replace,
    other_blockquote_setext_replace2,
    other_list_is_task,
    other_list_replace_task,
    other_list_task_checkbox,
    other_any_line,
    other_href_brackets,
    other_table_delimiter,
    other_table_align_chars,
    other_table_row_blank_line,
    other_table_align_right,
    other_table_align_center,
    other_table_align_left,
    other_start_a_tag,
    other_end_a_tag,
    other_start_pre_script_tag,
    other_end_pre_script_tag,
    other_start_angle_bracket,
    other_end_angle_bracket,
    other_unicode_alpha_numeric,
    other_numeric_character_reference,
    other_numeric_character_reference_or_ampersand,
    other_escape_test,
    other_escape_replace,
    other_escape_test_no_encode,
    other_escape_replace_no_encode,
    other_find_pipe,
    other_slash_pipe,
    other_carriage_return,
    other_not_space_start,
    other_ending_newline,
];
