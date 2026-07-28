#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/logs/archive" "${student_home}/lab/docs"
touch "${student_home}/lab/logs/app.log" "${student_home}/lab/logs/archive/old.log" "${student_home}/lab/docs/notes.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
