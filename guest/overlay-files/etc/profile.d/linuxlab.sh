# Interactive shell setup for the practice environment.
#
# Sourced for every login shell. Everything here is a default the learner is free to change:
# the curriculum teaches editing .bashrc, so nothing may be enforced from a place they cannot
# reach.

# The prompt the lessons refer to by name: student@linuxlab:~$
if [ -n "${BASH_VERSION}" ] && [ -n "${PS1}" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        # Root gets # rather than $, which lesson 0.2 asks the learner to notice.
        PS1='\[\e[1;31m\]\u@\h\[\e[0m\]:\[\e[1;34m\]\w\[\e[0m\]# '
    else
        PS1='\[\e[1;32m\]\u@\h\[\e[0m\]:\[\e[1;34m\]\w\[\e[0m\]$ '
    fi
    export PS1
fi

# Colour output makes ls -l readable in the terminal panel.
if [ -x /usr/bin/dircolors ]; then
    eval "$(dircolors -b)"
    alias ls='ls --color=auto'
    alias grep='grep --color=auto'
fi

# Command history stays inside the guest. The host records it only when the learner opts in,
# and this size is generous enough for Ctrl+R practice without being unbounded.
HISTSIZE=2000
HISTFILESIZE=4000
HISTCONTROL=ignoreboth
shopt -s histappend checkwinsize
export HISTSIZE HISTFILESIZE HISTCONTROL

# A learner who breaks PATH in lesson 7.3 needs a documented way back, so the original is kept.
export LINUXLAB_DEFAULT_PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

# Bash completion is part of the curriculum (lesson 1.7).
if ! shopt -oq posix && [ -f /usr/share/bash-completion/bash_completion ]; then
    . /usr/share/bash-completion/bash_completion
fi

# Deliberately no `alias rm='rm -i'`: the rm lesson teaches that rm does not ask, and an alias
# that quietly makes it safe would teach the wrong lesson.
