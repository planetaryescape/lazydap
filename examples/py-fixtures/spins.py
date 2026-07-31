"""Never stops on its own. The `state: timeout` case.

The sleep is not decoration: without it this burns a core for as long as a test
waits it out, and CI machines are shared.
"""

import time

print("spinning", flush=True)
while True:
    time.sleep(0.02)
