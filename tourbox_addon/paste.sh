#!/bin/bash

bash -euxc 'rm -f /tmp/xclip-workaround; sleep 0.2; xclip -selection clipboard -out | tee /tmp/xclip-workaround' &
disown
