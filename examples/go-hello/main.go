// Debuggee for M22 (the third adapter), the Go mirror of examples/py-hello.
//
// The line layout below is a contract, the same way c-hello's and py-hello's
// are: line 23 is the `y := x * 2` assignment, so a breakpoint there pauses
// with `x` defined and `y` not, after the first print has already arrived as an
// output event. Adding or removing lines above line 23 breaks that — update
// whatever set a breakpoint on it in the same commit.
//
// There is a `go.mod` beside this file because Delve's `debug` launch mode
// compiles the program before running it, and `go build` outside a module
// fails. Nothing needs building by hand: `lazydap launch
// examples/go-hello/main.go` compiles and runs it in one step.
//
// Run: lazydap launch examples/go-hello/main.go --stop-on-entry

package main

import "fmt"

func main() {
	x := 5
	fmt.Println("hello from m22")
	y := x * 2 // line 23 — breakpoint line
	fmt.Printf("goodbye y=%d\n", y)
}
