#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "start request\nuser=student\nERROR timeout\nretry scheduled\nrequest ended\n" > "${student_home}/lab/request.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
