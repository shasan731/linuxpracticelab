#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/data/subdir"
printf "small\n" > "${student_home}/lab/data/small.dat"
dd if=/dev/zero of="${student_home}/lab/data/large.dat" bs=1M count=2 status=none
chown -R student:student "${student_home}/lab" "${student_home}/answers"
