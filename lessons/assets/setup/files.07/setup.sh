#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/empty" "${student_home}/lab/old-cache/nested"
printf "cache\n" > "${student_home}/lab/old-cache/nested/item.tmp"
mkdir -p "${student_home}/lab/keep"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
