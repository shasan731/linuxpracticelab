#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
cat > "${student_home}/lab/auth.log" <<'LOG'
Failed password for invalid user admin from 10.20.0.5
Accepted publickey for student from 10.20.0.8
Failed password for root from 10.20.0.9
Failed password for invalid user admin from 10.20.0.5
LOG
chown -R student:student "${student_home}/lab" "${student_home}/answers"
