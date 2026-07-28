#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/cache"
touch "${student_home}/lab/cache/a.tmp" "${student_home}/lab/cache/old item.tmp"
printf "keep\n" > "${student_home}/lab/cache/keep.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
