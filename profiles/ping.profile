# Live ping RTT dial: ping <host> | termtaco --profile ping
# kalman-r is ping's mdev squared (mdev=35.868ms -> ~1300); kalman-q gives
# the filter a settling time of a few seconds. See the README's ping example.
parser = ping
title = ping ms
zero
kalman
kalman-q = 0.5
kalman-r = 1300
needle-inertia = 0.5
