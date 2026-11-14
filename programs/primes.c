// The prime numbers below 1000, by the sieve of Eratosthenes: go through
// the numbers in order; one that has not been crossed out is prime, and
// its multiples are crossed out.

#include "lib/console.h"

const u16 LIMIT = 1000;

u8 crossed[LIMIT];          // crossed[n] is 1 once n is known not to be prime

void main() {
    u16 count = 0;
    for (u16 n = 2; n < LIMIT; n++) {
        if (crossed[n]) continue;
        print_number(n);
        put(' ');
        count++;
        // Multiples below n * n have a smaller factor and are crossed out
        // already; from 32 up, n * n is beyond the limit.
        if (n < 32) {
            for (u16 m = n * n; m < LIMIT; m += n) crossed[m] = 1;
        }
    }
    put('\n');
    print_number(count);
    print(" primes below ");
    print_number(LIMIT);
    put('\n');
}
