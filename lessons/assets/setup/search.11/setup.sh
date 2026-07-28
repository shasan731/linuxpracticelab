#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "host=web1\nport=8080\n" > "${student_home}/lab/old.conf"
printf "host=web1\nport=9090\n" > "${student_home}/lab/new.conf"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
