#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/catalog"
printf "quarterly figures\n" > "${student_home}/lab/catalog/report.txt"
printf "remember hidden files\n" > "${student_home}/lab/catalog/.hidden-note"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
