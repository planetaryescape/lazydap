// Dies on an unrecovered panic. Go's answer to c-fixtures/crashes.c.
//
// There is no segfault to reach from safe Go, and an unrecovered panic is the
// same thing at this level: the program stops because of something it did, not
// because it was asked to, and the process exits non-zero (Go uses 2).

package main

import "fmt"

func main() {
	fmt.Println("about to fail")
	panic("nothing here")
}
