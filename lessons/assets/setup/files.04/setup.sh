#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/website/assets"
printf "<h1>Linux Lab</h1>\n" > "${student_home}/lab/website/index.html"
printf "theme=dark\n" > "${student_home}/lab/website/assets/site.conf"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
