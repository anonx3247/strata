// The tokens whose meaning depends on the layout, as Strata's parser reads
// them (`compiler/parse.strata`):
//
// - a call's `(` is written against what it calls: `f(x)` (`f (x)` is no call);
// - an index's `[`, a binary `-`, a postfix `!` or `?` are on the line of
//   what they follow (a line starting with one begins a new statement).
//
// Strata allows spaces before an index's `[` or a postfix `!`, but nobody
// writes them, and taking them as written against the value tells
// `return [x]`, `return !done` and `return -1` from an index, a `!` and a
// subtraction: an index's `[` and a postfix `!` or `?` are written against
// the value, and a `-` with a space before it and none after is a sign.

#include "tree_sitter/parser.h"

#include <stdbool.h>

enum TokenType {
  CALL_LPAREN,
  INDEX_LBRACKET,
  TRY_BANG,
  TRY_QUESTION,
  BINARY_MINUS,
  ERROR_SENTINEL,
};

void *tree_sitter_strata_external_scanner_create(void) { return NULL; }
void tree_sitter_strata_external_scanner_destroy(void *payload) {}
unsigned tree_sitter_strata_external_scanner_serialize(void *payload, char *buffer) { return 0; }
void tree_sitter_strata_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}

static bool is_word_start(int32_t c) {
  return c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c > 127;
}

bool tree_sitter_strata_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid) {
  // in error recovery every token is valid: let the grammar's own tokens be
  if (valid[ERROR_SENTINEL]) {
    return false;
  }
  // a `(` against what precedes it
  if (valid[CALL_LPAREN] && lexer->lookahead == '(') {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = CALL_LPAREN;
    return true;
  }
  if (!(valid[INDEX_LBRACKET] || valid[TRY_BANG] || valid[TRY_QUESTION] || valid[BINARY_MINUS])) {
    return false;
  }
  // spaces, but no newline
  bool space = false;
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t' || lexer->lookahead == '\r') {
    lexer->advance(lexer, true);
    space = true;
  }
  switch (lexer->lookahead) {
    case '[':
      if (!valid[INDEX_LBRACKET] || space) return false;
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      lexer->result_symbol = INDEX_LBRACKET;
      return true;
    case '!':
      if (!valid[TRY_BANG] || space) return false;
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      // `!=`, `!!` are operators
      if (lexer->lookahead == '=' || lexer->lookahead == '!') return false;
      lexer->result_symbol = TRY_BANG;
      return true;
    case '?':
      if (!valid[TRY_QUESTION] || space) return false;
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      // `??` is an operator, `?name` a hole
      if (lexer->lookahead == '?' || is_word_start(lexer->lookahead)) return false;
      lexer->result_symbol = TRY_QUESTION;
      return true;
    case '-':
      if (!valid[BINARY_MINUS]) return false;
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      // `-=`, `->` are other symbols
      if (lexer->lookahead == '=' || lexer->lookahead == '>') return false;
      // `f(x, -1)`, `return -n`: a sign
      if (space && !(lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
                     lexer->lookahead == '\r' || lexer->lookahead == '\n')) {
        return false;
      }
      lexer->result_symbol = BINARY_MINUS;
      return true;
    default:
      return false;
  }
}
