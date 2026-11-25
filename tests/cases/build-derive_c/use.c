// A C program calling what calc.strata exports (`derive C for f`), through
// the header `strata build --lib` made.
#include <stdio.h>
#include <stdlib.h>
#include "calc.h"

int main(void) {
  printf("%d\n", add(2, 40));
  char *s = greet("C");
  printf("%s\n", s);
  free(s);
  printf("%g\n", half(3.0));
  printf("%d %d\n", even(4), even(7));
  printf("%d\n", next_byte(255));
  fflush(stdout);
  hello();
  printf("%d\n", size("h\xc3\xa9llo"));
  return 0;
}
