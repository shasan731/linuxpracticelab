#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/archive"
printf "one\n" > "${student_home}/lab/report-1.txt"
printf "two\n" > "${student_home}/lab/report-2.txt"
printf "three\n" > "${student_home}/lab/report-3.txt"
printf "keep out\n" > "${student_home}/lab/report-final.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
