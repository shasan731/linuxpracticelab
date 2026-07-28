#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "alpha\n" > "${student_home}/lab/part-1.txt"
printf "beta\n" > "${student_home}/lab/part-2.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
