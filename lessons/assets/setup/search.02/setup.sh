#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "SUCCESS boot\nError disk\nwarning retry\nERROR network\nsuccess stop\n" > "${student_home}/lab/mixed.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
