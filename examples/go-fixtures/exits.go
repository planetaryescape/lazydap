// Runs to completion and exits cleanly. The `state: exited` case.
//
// Line 11 is the print, which is the breakpoint line `wait_delve.rs` uses.

package main

import "fmt"

func main() {
	x := 5
	fmt.Printf("about to finish x=%d\n", x)
}
