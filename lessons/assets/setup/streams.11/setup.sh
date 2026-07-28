#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "beta\nalpha\ngamma\n" > "${student_home}/lab/list-1.txt"
printf "gamma\nbeta\nalpha\n" > "${student_home}/lab/list-2.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
