#!/usr/bin/perl
# A host with tens of thousands of sockets — the second of the pathological environments spec §34
# asks performance tests to include.
#
# Unix domain sockets, because the acceptance container runs with networking disabled and a unix
# socket is the one kind that always exists. They are listening sockets with a queue, so they
# appear in `sock_diag` exactly as any other listener does. The file descriptor limit is per
# process, so the sockets are spread over a handful of children.
#
# Usage: many-sockets.pl <count> <directory> <state-file>
#
# Writes `FIXTURE_SOCKETS=<n>` into the state file once they exist.

use strict;
use warnings;
use IO::Socket::UNIX;
use POSIX ();

my $wanted    = shift // 5000;
my $directory = shift // '/tmp/many-sockets';
my $state     = shift // '/tmp/many-sockets.state';

mkdir $directory;

# 400 per child keeps every process well inside a 1024-descriptor limit, whatever the host sets.
my $per_child = 400;
my $children  = int(($wanted + $per_child - 1) / $per_child);

pipe(my $read, my $write) or die "pipe: $!";

my @pids;
my $made = 0;
for my $child (1 .. $children) {
    my $first = ($child - 1) * $per_child + 1;
    my $last  = $first + $per_child - 1;
    $last = $wanted if $last > $wanted;

    my $pid = fork();
    last if !defined $pid;
    if ($pid == 0) {
        close $write;
        my @held;
        for my $index ($first .. $last) {
            my $socket = IO::Socket::UNIX->new(
                Type   => IO::Socket::UNIX::SOCK_STREAM(),
                Local  => "$directory/socket-$index",
                Listen => 5,
            );
            last if !$socket;
            push @held, $socket;
        }
        # Report how many this child actually opened, then wait for the parent to go away.
        if (open(my $report, '>', "$directory/.count-$child")) {
            print {$report} scalar(@held), "\n";
            close $report;
        }
        my $ignored = <$read>;
        POSIX::_exit(0);
    }
    push @pids, $pid;
    $made += $last - $first + 1;
}
close $read;

# Wait until every child has reported, so the state file is true when the case reads it.
for my $attempt (1 .. 600) {
    my $reported = 0;
    my $files    = 0;
    for my $child (1 .. scalar(@pids)) {
        if (open(my $count, '<', "$directory/.count-$child")) {
            my $line = <$count>;
            close $count;
            next if !defined $line;
            chomp $line;
            $reported += $line;
            $files++;
        }
    }
    if ($files == scalar(@pids)) {
        $made = $reported;
        last;
    }
    select(undef, undef, undef, 0.1);
}

open(my $out, '>', $state) or die "cannot write $state: $!";
print {$out} "FIXTURE_SOCKETS=$made\n";
close $out;

$SIG{TERM} = sub { POSIX::_exit(0) };
$SIG{INT}  = sub { POSIX::_exit(0) };
sleep 3600;
