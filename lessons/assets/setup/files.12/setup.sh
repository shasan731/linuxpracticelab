#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/project/inbox"
printf "port=8080\n" > "${student_home}/lab/project/inbox/app.conf"
printf "quarter one\n" > "${student_home}/lab/project/inbox/draft-report.txt"
printf "discard\n" > "${student_home}/lab/project/inbox/cache.tmp"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
