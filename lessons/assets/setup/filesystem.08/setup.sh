#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/profile"
printf "visible\n" > "${student_home}/lab/profile/note.txt"
printf "hidden\n" > "${student_home}/lab/profile/.hidden-note"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
