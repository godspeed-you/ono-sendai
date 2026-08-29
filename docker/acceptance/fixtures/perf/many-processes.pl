#!/usr/bin/perl
# A host with tens of thousands of processes — the first of the pathological environments spec
# §34 asks performance tests to include.
#
# The children are forks of this interpreter rather than execs of `sleep`, for one reason: a fork
# shares its parent's pages until it writes, so ten thousand of them cost the machine a page table
# each instead of an image each. They exist to be *counted and read* by `/proc`, which is what the
# process provider does, and a forked child is indistinguishable from any other process there.
#
# Usage: many-processes.pl <count> <state-file>
#
# Writes `FIXTURE_PROCESSES=<n>` and `FIXTURE_PARENT=<pid>` into the state file once the children
# exist, so the case can report the number it actually reached rather than the number it wanted.
# Every child exits when the parent does, because the parent's exit closes the pipe they block on.

use strict;
use warnings;
use POSIX ();

my $wanted = shift // 10000;
my $state  = shift // '/tmp/many-processes.state';

pipe(my $read, my $write) or die "pipe: $!";

my $made = 0;
for my $index (1 .. $wanted) {
    my $pid = fork();
    if (!defined $pid) {
        # The machine said no more: that is the honest ceiling, and the case reports it.
        last;
    }
    if ($pid == 0) {
        close $write;
        # Block until the parent goes away. No timer, no wakeup, no CPU.
        my $ignored = <$read>;
        POSIX::_exit(0);
    }
    $made++;
}
close $read;

open(my $out, '>', $state) or die "cannot write $state: $!";
print {$out} "FIXTURE_PROCESSES=$made\n";
print {$out} "FIXTURE_PARENT=$$\n";
close $out;

# Stay alive holding the write end; the children block on the read end until this exits.
$SIG{TERM} = sub { POSIX::_exit(0) };
$SIG{INT}  = sub { POSIX::_exit(0) };
sleep 3600;
