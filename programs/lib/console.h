// Writing on the console.

#include "silicon.h"

void put(u8 c) {
    *OUT = c;
}

// A text: characters up to the zero that ends it.
void print(u8* text) {
    while (*text) put(*text++);
}

// A number, in decimal. The digits come out last first, so the ones before
// the last are printed by the function calling itself.
void print_number(u16 n) {
    if (n >= 10) print_number(n / 10);
    put('0' + (u8)(n % 10));
}
