#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "discard\n" > "${student_home}/lab/remove-me.txt"
printf "keep\n" > "${student_home}/lab/keep-me.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
