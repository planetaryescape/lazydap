// Prints a great deal, then stops. For captured_output volume and ordering.

package main

import "fmt"

func main() {
	for i := 0; i < 200; i++ {
		fmt.Printf("line %d\n", i)
	}
	done := 1
	fmt.Println("finished")
	_ = done
}
