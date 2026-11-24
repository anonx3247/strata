fn main() {
    for s in ["hello World", "straße", "ΟΔΥΣΣΕΥΣ", "İstanbul", "ǅ", "ﬃ", "123", "١٢٣", "  \t", "", "abc1", "日本語", "Ünïcödé"] {
        print(f"{s}: {to_upper(s)} {to_lower(s)} {is_digit(s)} {is_alpha(s)} {is_space(s)}")
    }
}
