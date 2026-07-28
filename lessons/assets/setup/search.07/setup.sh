#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/time"
touch -d "2 days ago" "${student_home}/lab/time/old.log"
touch -d "5 minutes ago" "${student_home}/lab/time/recent.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
