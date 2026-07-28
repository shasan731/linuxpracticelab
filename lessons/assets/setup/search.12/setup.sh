#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "INFO start\nERROR disk\nERROR auth\nINFO retry\nERROR disk\n" > "${student_home}/lab/app.log"
printf "port=8080\nmode=prod\n" > "${student_home}/lab/old.conf"
printf "port=9090\nmode=prod\n" > "${student_home}/lab/new.conf"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
