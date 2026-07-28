#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/project/api" "${student_home}/lab/project/worker"
printf "database_url=sqlite:///api.db\n" > "${student_home}/lab/project/api/app.env"
printf "threads=4\n" > "${student_home}/lab/project/worker/worker.env"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
