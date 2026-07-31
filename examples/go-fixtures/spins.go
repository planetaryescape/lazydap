// Never stops on its own. The `state: timeout` case.
//
// The sleep is not decoration: without it this burns a core for as long as a
// test waits it out, and CI machines are shared.

package main

import (
	"fmt"
	"time"
)

func main() {
	fmt.Println("spinning")
	for {
		time.Sleep(20 * time.Millisecond)
	}
}
